//! Implementation of the `agrm solve` command.

use clap::Args;
use owo_colors::OwoColorize;
use std::path::PathBuf;
use std::time::Instant;

use crate::cli::init::{InitArgs, run as run_init};
use crate::cli::resolve_db_path;
use crate::format::reader::Reader;
use crate::ui::{
  COLUMN_SPACING, MAX_LAYOUT_WIDTH, detect_terminal_width,
  format_result_summary, format_word_columns, format_word_grid,
};

/// Arguments for the `agrm solve` command.
#[derive(Args)]
pub struct SolveArgs {
  /// Word or letters to solve (e.g. "listen", "cat").
  #[arg(value_name = "QUERY")]
  pub query: String,

  /// Path to the .agrm database (defaults to $TMP/english_words.agrm).
  #[arg(short, long, value_name = "PATH")]
  pub db: Option<PathBuf>,

  /// Find only exact anagrams of the entire query (by default, sub-anagrams are returned).
  #[arg(short = 'e', long)]
  pub exact: bool,

  /// Deprecated alias; sub-anagrams are now searched by default.
  #[arg(short, long, default_value_t = true)]
  pub sub: bool,

  /// Minimum word length when searching for sub-anagrams (defaults to 3).
  #[arg(short = 'm', long, default_value_t = 3)]
  pub min_len: usize,

  /// Limit the maximum number of results printed.
  #[arg(short = 'n', long, value_name = "COUNT")]
  pub limit: Option<usize>,

  /// Show detailed microsecond timing breakdown (read, solve, display).
  #[arg(short = 't', long)]
  pub timing: bool,

  /// Output results as a raw single-column wordlist without headers or colors (default when piped).
  #[arg(long)]
  pub raw: bool,

  /// Output results as a JSON array.
  #[arg(long)]
  pub json: bool,

  /// Fast open (skips full CRC32 payload verification for microsecond startup).
  #[arg(long, default_value_t = true)]
  pub fast: bool,
}

/// Executes the `solve` command.
pub fn run(args: SolveArgs) -> crate::Result<()> {
  let total_start = Instant::now();
  let db_path = resolve_db_path(args.db);

  // Auto-init if database does not exist
  if !db_path.exists() {
    eprintln!("Initializing default dictionary at {:?}...", db_path);
    let init_args = InitArgs {
      sources: Vec::new(),
      output: Some(db_path.clone()),
      force: false,
    };
    run_init(init_args)?;
  }

  // 1. Open database via mmap (fast or full)
  let read_start = Instant::now();
  let reader = if args.fast {
    Reader::open_fast(&db_path)?
  } else {
    Reader::open(&db_path)?
  };
  let read_elapsed = read_start.elapsed();

  let use_raw = args.raw;

  use std::io::Write;
  let stdout = std::io::stdout();
  let mut out = std::io::BufWriter::new(stdout.lock());

  let solve_start = Instant::now();

  // If exact anagrams requested:
  if args.exact {
    let mut words: Vec<&str> = reader.anagrams(&args.query).collect();
    words.sort_unstable();
    let solve_elapsed = solve_start.elapsed();

    if let Some(limit) = args.limit {
      words.truncate(limit);
    }

    let format_start = Instant::now();
    let count = words.len();

    if args.json {
      write_json(&mut out, &words)?;
      writeln!(out)?;
      out.flush()?;
    } else if use_raw {
      for word in &words {
        writeln!(out, "{word}")?;
      }
      out.flush()?;
    } else {
      let term_width = detect_terminal_width();
      if count > 0 {
        let grid = format_word_grid(&words, term_width, true);
        write!(out, "{grid}")?;
      }
      let format_elapsed = format_start.elapsed();
      let total_elapsed = total_start.elapsed();

      writeln!(out, "{}", format_result_summary(count, total_elapsed))?;
      if args.timing {
        writeln!(
          out,
          "{}",
          format!(
            "  read: {read_elapsed:?} | solve: {solve_elapsed:?} | display: {format_elapsed:?}"
          )
          .dimmed()
        )?;
      }
      out.flush()?;
    }
    return Ok(());
  }

  // Default: Sub-anagrams
  if args.json {
    let mut words = reader.sub_anagrams(&args.query, args.min_len);
    words.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    if let Some(limit) = args.limit {
      words.truncate(limit);
    }
    write_json(&mut out, &words)?;
    writeln!(out)?;
    out.flush()?;
    return Ok(());
  }

  if use_raw {
    // Stream raw word-by-word with zero buffering overhead
    let mut count = 0usize;
    let limit = args.limit.unwrap_or(usize::MAX);
    reader.sub_anagrams_streaming(&args.query, args.min_len, |word| {
      if count < limit {
        let _ = writeln!(out, "{word}");
        count += 1;
      }
    });
    out.flush()?;
    return Ok(());
  }

  // Structured, well-spaced table layout grouped by word length descending
  let grouped = reader.sub_anagrams_grouped(&args.query, args.min_len);
  let solve_elapsed = solve_start.elapsed();

  let format_start = Instant::now();
  let term_width = detect_terminal_width().min(MAX_LAYOUT_WIDTH);
  let mut total_words = 0usize;
  let limit = args.limit.unwrap_or(usize::MAX);

  let max_len = grouped
    .iter()
    .flat_map(|(_, words)| words.iter().map(|w| w.len()))
    .max()
    .unwrap_or(0);
  let col_width = max_len + COLUMN_SPACING;
  let num_cols = (term_width / col_width).max(1);
  let longest_len = grouped.first().map(|(len, _)| *len).unwrap_or(0);

  let mut accumulated = 0usize;
  for (len, words) in &grouped {
    total_words += words.len();
    if accumulated < limit {
      let remaining = limit - accumulated;
      let slice = if words.len() <= remaining {
        accumulated += words.len();
        words.as_slice()
      } else {
        accumulated += remaining;
        &words[..remaining]
      };

      let highlight = *len == longest_len;
      let chunk = format_word_columns(slice, col_width, num_cols, highlight);
      write!(out, "{chunk}")?;
      out.flush()?; // Stream each length section progressively to stdout!
    }
  }

  let format_elapsed = format_start.elapsed();
  let total_elapsed = total_start.elapsed();

  writeln!(out, "{}", format_result_summary(total_words, total_elapsed))?;
  if args.timing {
    writeln!(
      out,
      "{}",
      format!(
        "  read: {read_elapsed:?} | solve: {solve_elapsed:?} | display: {format_elapsed:?}"
      )
      .dimmed()
    )?;
  }
  out.flush()?;

  Ok(())
}

fn write_json<W: std::io::Write>(
  writer: &mut W,
  words: &[&str],
) -> std::io::Result<()> {
  writer.write_all(b"[")?;
  for (i, word) in words.iter().enumerate() {
    if i > 0 {
      writer.write_all(b",")?;
    }
    writer.write_all(b"\"")?;
    writer.write_all(word.as_bytes())?;
    writer.write_all(b"\"")?;
  }
  writer.write_all(b"]")
}
