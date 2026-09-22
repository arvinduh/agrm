//! Naive sub-anagram baselines to compare against the trie in `benchmark.rs`.
//!
//! Uses the same dictionary, queries, round counts, and CSV schema as
//! `benchmark.rs`, so `compare.py` can read both. Both baselines hold the
//! word list in memory and scan all of it on every query:
//!
//! - `scan`: counts each word's letters on the fly and checks them against the
//!   rack, bailing out on the first letter the rack cannot supply.
//! - `hist`: precomputes every word's letter histogram and 26-bit presence mask
//!   at load time, then rejects words by length and mask before comparing
//!   counts.
//!
//! Neither baseline beats the trie on realistic racks. On pathological racks
//! (roughly 50+ letters, where most of the dictionary matches) the trie visits
//! nearly every node and the sequential `scan` becomes faster.

use std::fs::{self, File};
use std::hint::black_box;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;

use anagram::DEFAULT_DICTIONARY_URL;

const MIN_LEN: usize = 3;
const QUERIES: [&str; 10] = [
  "cat",
  "stop",
  "apple",
  "listen",
  "roaster",
  "creative",
  "algorithms",
  "relationship",
  "conversational",
  "characteristically",
];
const CSV_HEADER: &str =
  "op,target,rounds,iterations,mean_s,min_s,max_s,median_s,stddev_s\n";

#[derive(Parser, Debug)]
#[command(about = "Benchmark naive sub-anagram baselines")]
struct Args {
  /// Flag automatically passed by `cargo bench`
  #[arg(long, hide = true)]
  bench: bool,

  /// Directory to save `rust_naive_scan.csv` and `rust_naive_hist.csv` in
  /// (defaults to benches/data if flag is passed without path)
  #[arg(long, short, num_args = 0..=1, default_missing_value = "benches/data")]
  save: Option<PathBuf>,
}

/// A word with its letter histogram and presence mask precomputed.
struct Entry<'a> {
  word: &'a str,
  len: usize,
  mask: u32,
  counts: [u8; 26],
}

struct Stats {
  mean_s: f64,
  min_s: f64,
  max_s: f64,
  median_s: f64,
  stddev_s: f64,
}

fn letter_counts(s: &str) -> Option<[u8; 26]> {
  let mut counts = [0u8; 26];
  for b in s.bytes() {
    let lower = b | 0x20;
    if !lower.is_ascii_lowercase() {
      return None;
    }
    counts[(lower - b'a') as usize] += 1;
  }
  Some(counts)
}

fn mask_of(counts: &[u8; 26]) -> u32 {
  (0..26)
    .filter(|&i| counts[i] > 0)
    .fold(0, |mask, i| mask | (1 << i))
}

fn load_words(text: &str) -> Vec<&str> {
  text
    .split(['\n', '\r'])
    .map(str::trim)
    .filter(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphabetic()))
    .collect()
}

fn build_index<'a>(words: &[&'a str]) -> Vec<Entry<'a>> {
  words
    .iter()
    .filter_map(|&word| {
      let counts = letter_counts(word)?;
      Some(Entry {
        word,
        len: word.len(),
        mask: mask_of(&counts),
        counts,
      })
    })
    .collect()
}

fn sub_anagrams_scan<'a>(words: &[&'a str], rack: &str) -> Vec<&'a str> {
  let Some(rack_counts) = letter_counts(rack) else {
    return Vec::new();
  };

  let mut results = Vec::new();
  for &word in words {
    if word.len() < MIN_LEN || word.len() > rack.len() {
      continue;
    }

    let mut remaining = rack_counts;
    let fits = word.bytes().all(|b| {
      let slot = &mut remaining[((b | 0x20) - b'a') as usize];
      let available = *slot > 0;
      *slot = slot.saturating_sub(1);
      available
    });
    if fits {
      results.push(word);
    }
  }
  results
}

fn sub_anagrams_hist<'a>(index: &[Entry<'a>], rack: &str) -> Vec<&'a str> {
  let Some(rack_counts) = letter_counts(rack) else {
    return Vec::new();
  };
  let rack_mask = mask_of(&rack_counts);

  index
    .iter()
    .filter(|e| e.len >= MIN_LEN && e.len <= rack.len())
    .filter(|e| e.mask & !rack_mask == 0)
    .filter(|e| e.counts.iter().zip(&rack_counts).all(|(w, r)| w <= r))
    .map(|e| e.word)
    .collect()
}

fn compute_stats(mut times: Vec<f64>) -> Stats {
  times.sort_by(|a, b| a.partial_cmp(b).unwrap());
  let n = times.len() as f64;
  let mean_s = times.iter().sum::<f64>() / n;
  let variance = times.iter().map(|t| (t - mean_s).powi(2)).sum::<f64>() / n;
  Stats {
    mean_s,
    min_s: times[0],
    max_s: times[times.len() - 1],
    median_s: times[times.len() / 2],
    stddev_s: variance.sqrt(),
  }
}

fn rounds_for(len: usize) -> u32 {
  if len <= 7 {
    50
  } else if len <= 10 {
    20
  } else {
    10
  }
}

/// Runs `f` once to warm up, then `rounds` timed times.
fn time_rounds<T>(rounds: u32, mut f: impl FnMut() -> T) -> (Stats, T) {
  let mut last = f();
  let mut times = Vec::with_capacity(rounds as usize);
  for _ in 0..rounds {
    let t0 = Instant::now();
    last = black_box(f());
    times.push(t0.elapsed().as_secs_f64());
  }
  (compute_stats(times), last)
}

fn csv_row(op: &str, target: &str, rounds: u32, s: &Stats) -> String {
  format!(
    "{op},{target},{rounds},1,{:.9},{:.9},{:.9},{:.9},{:.9}\n",
    s.mean_s, s.min_s, s.max_s, s.median_s, s.stddev_s
  )
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

fn write_csv(dir: &Path, name: &str, body: &str) {
  fs::create_dir_all(dir).expect("failed to create output directory");
  let path = dir.join(name);
  File::create(&path)
    .and_then(|mut f| f.write_all(body.as_bytes()))
    .expect("failed to write CSV");
  println!("Saved {}", path.display());
}

fn main() {
  let args = Args::parse();
  let raw_dict = ensure_raw_dictionary();

  // Parse: read and split for `scan`; also build histograms for `hist`.
  let (parse_scan, _) = time_rounds(5, || {
    let text = fs::read_to_string(&raw_dict).expect("failed to read dict");
    load_words(&text).len()
  });
  let (parse_hist, _) = time_rounds(5, || {
    let text = fs::read_to_string(&raw_dict).expect("failed to read dict");
    build_index(&load_words(&text)).len()
  });

  let text = fs::read_to_string(&raw_dict).expect("failed to read dict");
  let words = load_words(&text);
  let index = build_index(&words);

  let mut scan_csv = String::from(CSV_HEADER);
  let mut hist_csv = String::from(CSV_HEADER);
  scan_csv += &csv_row("parse", "dwyl_english", 5, &parse_scan);
  hist_csv += &csv_row("parse", "dwyl_english", 5, &parse_hist);

  println!("{} words loaded", words.len());
  println!(
    "{:>4}  {:<20} {:>8} {:>12} {:>12}",
    "len", "query", "matches", "scan", "hist"
  );
  for query in QUERIES {
    let rounds = rounds_for(query.len());
    let (scan, scan_words) =
      time_rounds(rounds, || sub_anagrams_scan(&words, query));
    let (hist, hist_words) =
      time_rounds(rounds, || sub_anagrams_hist(&index, query));
    assert_eq!(scan_words, hist_words, "baselines disagree on {query}");

    println!(
      "{:>4}  {:<20} {:>8} {:>9.1} µs {:>9.1} µs",
      query.len(),
      query,
      scan_words.len(),
      scan.mean_s * 1e6,
      hist.mean_s * 1e6,
    );
    scan_csv += &csv_row("query", query, rounds, &scan);
    hist_csv += &csv_row("query", query, rounds, &hist);
  }

  if let Some(dir) = args.save {
    write_csv(&dir, "rust_naive_scan.csv", &scan_csv);
    write_csv(&dir, "rust_naive_hist.csv", &hist_csv);
  }
}
