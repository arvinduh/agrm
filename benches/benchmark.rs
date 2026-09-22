//! Benchmark harness for `anagram`.
//!
//! Measures dictionary ingestion (`parse`) and sub-anagram lookup (`query`)
//! latency, rendering clean terminal tables with optional CSV export for comparisons.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, CellAlignment, Table};

use anagram::ui::{format_count, render_bar};
use anagram::{
  DEFAULT_DICTIONARY_URL, Reader, Source, Writer, default_cache_path, ingest,
};

#[derive(Parser, Debug)]
#[command(about = "Benchmark anagram solver ingestion and queries")]
struct Args {
  /// Flag automatically passed by `cargo bench`
  #[arg(long, hide = true)]
  bench: bool,

  /// Path to save CSV benchmark results (defaults to benches/data/rust.csv if flag is passed without path)
  #[arg(
    long,
    short,
    alias = "csv",
    alias = "benchmark-csv",
    num_args = 0..=1,
    default_missing_value = "benches/data/rust.csv"
  )]
  save: Option<PathBuf>,
}

struct BenchmarkResult {
  op: &'static str,
  target: &'static str,
  length: usize,
  items_count: usize,
  rounds: u32,
  mean_s: f64,
  min_s: f64,
  max_s: f64,
  median_s: f64,
  stddev_s: f64,
  throughput_wps: f64,
}

fn compute_stats(mut times: Vec<f64>) -> (f64, f64, f64, f64, f64) {
  times.sort_by(|a, b| a.partial_cmp(b).unwrap());
  let n = times.len() as f64;
  let min_s = times[0];
  let max_s = times[times.len() - 1];
  let median_s = times[times.len() / 2];
  let mean_s = times.iter().sum::<f64>() / n;
  let variance = times.iter().map(|t| (t - mean_s).powi(2)).sum::<f64>() / n;
  let stddev_s = variance.sqrt();
  (mean_s, min_s, max_s, median_s, stddev_s)
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

fn ensure_benchmark_database(raw_dict: &PathBuf) -> PathBuf {
  let path = default_cache_path();
  if !path.exists() {
    let sources = vec![Source::Local(raw_dict.clone())];
    let trie = ingest(&sources, |&b| b == b'\n' || b == b'\r');
    Writer::build_from_trie(&path, &trie)
      .expect("failed to build benchmark database");
  }
  path
}

fn write_csv(
  path: &PathBuf,
  results: &[BenchmarkResult],
) -> std::io::Result<()> {
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent)?;
  }
  let mut file = File::create(path)?;
  writeln!(
    file,
    "op,target,rounds,iterations,mean_s,min_s,max_s,median_s,stddev_s"
  )?;
  for r in results {
    writeln!(
      file,
      "{},{},{},1,{:.9},{:.9},{:.9},{:.9},{:.9}",
      r.op,
      r.target,
      r.rounds,
      r.mean_s,
      r.min_s,
      r.max_s,
      r.median_s,
      r.stddev_s
    )?;
  }
  Ok(())
}

fn main() {
  let args = Args::parse();
  let raw_dict = ensure_raw_dictionary();
  let db_path = ensure_benchmark_database(&raw_dict);

  let mut all_results = Vec::new();

  // 1. Ingestion / Parse Benchmark (matching Python dwyl_english 5 rounds)
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

  let (mean_s, min_s, max_s, median_s, stddev_s) = compute_stats(parse_times);
  let parse_wps = if mean_s > 0.0 {
    total_words as f64 / mean_s
  } else {
    0.0
  };

  let parse_result = BenchmarkResult {
    op: "parse",
    target: "dwyl_english",
    length: 12,
    items_count: total_words,
    rounds: parse_rounds,
    mean_s,
    min_s,
    max_s,
    median_s,
    stddev_s,
    throughput_wps: parse_wps,
  };

  // 2. Query Lookups Benchmark
  let reader = Reader::open_fast(&db_path).expect("failed to open database");
  let queries = [
    (3, "cat"),
    (4, "stop"),
    (5, "apple"),
    (6, "listen"),
    (7, "roaster"),
    (8, "creative"),
    (10, "algorithms"),
    (12, "relationship"),
    (15, "conversational"),
    (18, "characteristically"),
  ];

  let mut query_results = Vec::new();
  let mut max_query_micros = 0.0f64;

  for (len, query) in queries {
    // Warmup
    let _ = reader.sub_anagrams(query, 3);

    let rounds = if len <= 7 {
      50
    } else if len <= 10 {
      20
    } else {
      10
    };

    let mut times = Vec::with_capacity(rounds as usize);
    let mut words_count = 0;
    for _ in 0..rounds {
      let t0 = Instant::now();
      let words = reader.sub_anagrams(query, 3);
      times.push(t0.elapsed().as_secs_f64());
      words_count = words.len();
    }

    let (mean_s, min_s, max_s, median_s, stddev_s) = compute_stats(times);
    let micros = mean_s * 1_000_000.0;
    if micros > max_query_micros {
      max_query_micros = micros;
    }

    let throughput_wps = if mean_s > 0.0 {
      words_count as f64 / mean_s
    } else {
      0.0
    };

    query_results.push(BenchmarkResult {
      op: "query",
      target: query,
      length: len,
      items_count: words_count,
      rounds,
      mean_s,
      min_s,
      max_s,
      median_s,
      stddev_s,
      throughput_wps,
    });
  }

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
    Cell::new(parse_result.op),
    Cell::new(parse_result.target),
    Cell::new(format_count(parse_result.items_count))
      .set_alignment(CellAlignment::Right),
    Cell::new(format!(
      "{:?}",
      Duration::from_secs_f64(parse_result.mean_s)
    ))
    .set_alignment(CellAlignment::Right),
    Cell::new(format!("{:.0} w/s", parse_result.throughput_wps))
      .set_alignment(CellAlignment::Right),
  ]);

  println!("{parse_table}");
  println!();

  // Render Query Table
  let mut query_table = Table::new();
  query_table
    .load_style(UTF8_FULL.with_rounded_corners())
    .set_header(vec![
      Cell::new("Length").set_alignment(CellAlignment::Center),
      Cell::new("Query Sample"),
      Cell::new("Words Found").set_alignment(CellAlignment::Right),
      Cell::new("Solve Time").set_alignment(CellAlignment::Right),
      Cell::new("Throughput").set_alignment(CellAlignment::Right),
      Cell::new("Latency Chart"),
    ]);

  for r in &query_results {
    let micros = r.mean_s * 1_000_000.0;
    let bar = render_bar(micros, max_query_micros, 20);
    let time_str = format!("{:?}", Duration::from_secs_f64(r.mean_s));
    let wps_str = format!("{:.0} w/s", r.throughput_wps);

    query_table.add_row(vec![
      Cell::new(r.length).set_alignment(CellAlignment::Center),
      Cell::new(r.target),
      Cell::new(r.items_count).set_alignment(CellAlignment::Right),
      Cell::new(time_str).set_alignment(CellAlignment::Right),
      Cell::new(wps_str).set_alignment(CellAlignment::Right),
      Cell::new(bar),
    ]);
  }

  println!("{query_table}");

  all_results.push(parse_result);
  all_results.extend(query_results);

  if let Some(csv_path) = args.save {
    if let Err(e) = write_csv(&csv_path, &all_results) {
      eprintln!("Failed to write CSV: {e}");
    }
  }
}
