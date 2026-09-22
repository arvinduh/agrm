//! Benchmark harness for `anagram`.
//!
//! Measures dictionary ingestion (`parse`) and sub-anagram lookup latency under
//! the `query`, `mixed`, and `startup` workloads defined in `common`, rendering
//! clean terminal tables with optional CSV export for comparisons.

mod common;

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, CellAlignment, Table};

use anagram::ui::{format_count, render_bar};
use anagram::{
  DEFAULT_DICTIONARY_URL, Reader, Source, Writer, default_cache_path, ingest,
};
use common::{CSV_HEADER, CacheEvictor, MIN_LEN, Row, compute_stats};

#[derive(Parser, Debug)]
#[command(about = "Benchmark anagram solver ingestion and queries")]
struct Args {
  /// Flag automatically passed by `cargo bench`
  #[arg(long, hide = true)]
  bench: bool,

  /// Path to save CSV benchmark results (defaults to benches/data/rust_trie.csv if flag is passed without path)
  #[arg(
    long,
    short,
    alias = "csv",
    alias = "benchmark-csv",
    num_args = 0..=1,
    default_missing_value = "benches/data/rust_trie.csv"
  )]
  save: Option<PathBuf>,
}

fn ensure_raw_dictionary() -> PathBuf {
  let path = std::env::temp_dir().join("words_alpha.txt");
  if !path.exists() {
    let _ = anagram::ingest::fetch_remote_to_file(
      DEFAULT_DICTIONARY_URL,
      &path,
      500 * 1024 * 1024,
    );
  }
  path
}

fn ensure_benchmark_database(raw_dict: &Path) -> PathBuf {
  let path = default_cache_path();
  if !path.exists() {
    let sources = vec![Source::Local(raw_dict.to_path_buf())];
    let trie = ingest(&sources, |&b| b == b'\n' || b == b'\r');
    Writer::build_from_trie(&path, &trie)
      .expect("failed to build benchmark database");
  }
  path
}

fn write_csv(path: &Path, rows: &[Row]) -> std::io::Result<()> {
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent)?;
  }
  let mut file = File::create(path)?;
  writeln!(file, "{CSV_HEADER}")?;
  for row in rows {
    writeln!(file, "{}", row.to_csv())?;
  }
  Ok(())
}

fn words_per_sec(items: usize, secs: f64) -> String {
  let wps = if secs > 0.0 { items as f64 / secs } else { 0.0 };
  format!("{wps:.0} w/s")
}

fn duration_cell(secs: f64) -> Cell {
  Cell::new(format!("{:?}", Duration::from_secs_f64(secs)))
    .set_alignment(CellAlignment::Right)
}

fn main() {
  let args = Args::parse();
  let raw_dict = ensure_raw_dictionary();
  let db_path = ensure_benchmark_database(&raw_dict);

  // 1. Ingestion / Parse Benchmark
  let parse_rounds = 5;
  let mut parse_times = Vec::with_capacity(parse_rounds as usize);
  let mut total_words = 0;
  let tmp_db = std::env::temp_dir()
    .join(format!("bench_parse_{}.tmp", std::process::id()));

  for _ in 0..parse_rounds {
    let t0 = Instant::now();
    let trie = ingest(&[Source::Local(raw_dict.clone())], |&b| {
      b == b'\n' || b == b'\r'
    });
    Writer::build_from_trie(&tmp_db, &trie)
      .expect("failed to serialize database");
    parse_times.push(t0.elapsed().as_secs_f64());
    total_words = trie.total_words();
    let _ = std::fs::remove_file(&tmp_db);
  }

  let parse = Row {
    op: "parse",
    target: "dwyl_english",
    rounds: parse_rounds,
    items_count: total_words,
    stats: compute_stats(parse_times),
  };

  // 2. Query Benchmarks: `query` and `mixed` against one open reader, then
  // `startup`, which reopens the database for every call.
  let reader = Reader::open_fast(&db_path).expect("failed to open database");
  let hot = common::run_hot(|rack| reader.sub_anagrams(rack, MIN_LEN).len());
  let startup = common::run_startup(&mut CacheEvictor::new(), |rack| {
    Reader::open_fast(&db_path)
      .expect("failed to open database")
      .sub_anagrams(rack, MIN_LEN)
      .len()
  });

  // Render Parse Table
  let mut parse_table = Table::new();
  parse_table
    .load_style(UTF8_FULL.with_rounded_corners())
    .set_header(vec![
      Cell::new("Operation"),
      Cell::new("Word Bank"),
      Cell::new("Words Ingested").set_alignment(CellAlignment::Right),
      Cell::new("Parse Time").set_alignment(CellAlignment::Right),
      Cell::new("Throughput").set_alignment(CellAlignment::Right),
    ]);

  parse_table.add_row(vec![
    Cell::new(parse.op),
    Cell::new(parse.target),
    Cell::new(format_count(parse.items_count))
      .set_alignment(CellAlignment::Right),
    duration_cell(parse.stats.mean_s),
    Cell::new(words_per_sec(parse.items_count, parse.stats.mean_s))
      .set_alignment(CellAlignment::Right),
  ]);

  println!("{parse_table}");
  println!();

  // Render Query Table: one row per rack, one column per workload.
  let by_op = |op: &str| -> Vec<&Row> {
    hot.iter().chain(&startup).filter(|r| r.op == op).collect()
  };
  let (query, mixed, cold) = (by_op("query"), by_op("mixed"), by_op("startup"));
  let max_query_micros = query
    .iter()
    .map(|r| r.stats.mean_s * 1_000_000.0)
    .fold(0.0, f64::max);

  let mut query_table = Table::new();
  query_table
    .load_style(UTF8_FULL.with_rounded_corners())
    .set_header(vec![
      Cell::new("Length").set_alignment(CellAlignment::Center),
      Cell::new("Query Sample"),
      Cell::new("Words Found").set_alignment(CellAlignment::Right),
      Cell::new("Query").set_alignment(CellAlignment::Right),
      Cell::new("Mixed").set_alignment(CellAlignment::Right),
      Cell::new("Startup").set_alignment(CellAlignment::Right),
      Cell::new("Throughput").set_alignment(CellAlignment::Right),
      Cell::new("Latency Chart"),
    ]);

  for ((q, m), c) in query.iter().zip(&mixed).zip(&cold) {
    let micros = q.stats.mean_s * 1_000_000.0;
    query_table.add_row(vec![
      Cell::new(q.target.len()).set_alignment(CellAlignment::Center),
      Cell::new(q.target),
      Cell::new(q.items_count).set_alignment(CellAlignment::Right),
      duration_cell(q.stats.mean_s),
      duration_cell(m.stats.mean_s),
      duration_cell(c.stats.mean_s),
      Cell::new(words_per_sec(q.items_count, q.stats.mean_s))
        .set_alignment(CellAlignment::Right),
      Cell::new(render_bar(micros, max_query_micros, 20)),
    ]);
  }

  println!("{query_table}");

  if let Some(csv_path) = args.save {
    let rows: Vec<Row> =
      std::iter::once(parse).chain(hot).chain(startup).collect();
    if let Err(e) = write_csv(&csv_path, &rows) {
      eprintln!("Failed to write CSV: {e}");
    }
  }
}
