//! Implementation of `agrm init`.

use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use std::path::PathBuf;
use std::time::Instant;

use crate::commands::resolve_db_path;
use crate::format::writer::Writer;
use crate::ingest::{DEFAULT_DICTIONARY_URL, Source, ingest};
use crate::ui::format_count;

/// Arguments for the `agrm init` command.
#[derive(Args)]
pub struct InitArgs {
  /// Input wordlist files (paths or URLs). Defaults to standard English dictionary if omitted.
  #[arg(value_name = "SOURCES")]
  pub sources: Vec<String>,

  /// Destination database file path (defaults to $TMP/english_words.agrm).
  #[arg(short, long, value_name = "PATH")]
  pub output: Option<PathBuf>,

  /// Overwrite existing database file without confirmation.
  #[arg(short, long)]
  pub force: bool,

  /// Silence progress and summary output.
  #[arg(short, long)]
  pub quiet: bool,
}

/// Executes the `init` command.
pub fn run(
  args: InitArgs,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let total_start = Instant::now();
  let output_path = resolve_db_path(args.output);

  if output_path.exists() && !args.force {
    if !args.quiet {
      eprintln!(
        "{} {}",
        "Database already exists".dimmed(),
        "(use --force to overwrite)".yellow()
      );
    }
    return Ok(());
  }

  let source_strings = if args.sources.is_empty() {
    vec![DEFAULT_DICTIONARY_URL.to_string()]
  } else {
    args.sources
  };

  let sources: Vec<Source> = source_strings
    .iter()
    .map(|s| Source::from_str_or_url(s.as_str()))
    .collect();

  let spinner = if !args.quiet {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
      ProgressStyle::default_spinner()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
        .template("{spinner:.cyan} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message("Ingesting dictionary...");
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    Some(pb)
  } else {
    None
  };

  let trie = ingest(&sources, |&b| b == b'\n' || b == b'\r');

  if let Some(ref pb) = spinner {
    pb.set_message("Serializing database...");
  }

  Writer::build_from_trie(&output_path, &trie)?;

  if let Some(pb) = spinner {
    pb.finish_and_clear();
  }

  if !args.quiet {
    let file_size = std::fs::metadata(&output_path)?.len();
    let file_size_mb = file_size as f64 / (1024.0 * 1024.0);
    let total_elapsed = total_start.elapsed();
    let count_str = format_count(trie.total_words());
    let size_str = format!("{:.2} MB", file_size_mb);
    let time_str = format!("{total_elapsed:?}");
    eprintln!(
      "{} {}  {}  {}",
      count_str.bold(),
      "words".dimmed(),
      size_str.dimmed(),
      time_str.cyan()
    );
  }

  Ok(())
}
