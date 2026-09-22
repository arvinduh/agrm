//! Naive sub-anagram baselines to compare against the trie in `benchmark.rs`.
//!
//! Uses the same dictionary, racks, workloads, and CSV schema as
//! `benchmark.rs` (see `common`), so `compare.py` can read both. Both
//! baselines hold the word list in memory and scan all of it on every query:
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

mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Parser;

use anagram::DEFAULT_DICTIONARY_URL;
use common::{CSV_HEADER, CacheEvictor, MIN_LEN, Row, time_rounds};

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

fn read_dict(path: &Path) -> String {
  fs::read_to_string(path).expect("failed to read dictionary")
}

fn write_csv(dir: &Path, name: &str, rows: &[Row]) {
  fs::create_dir_all(dir).expect("failed to create output directory");
  let path = dir.join(name);
  let mut body = format!("{CSV_HEADER}\n");
  for row in rows {
    body += &row.to_csv();
    body.push('\n');
  }
  File::create(&path)
    .and_then(|mut f| f.write_all(body.as_bytes()))
    .expect("failed to write CSV");
  println!("Saved {}", path.display());
}

fn print_table(scan: &[Row], hist: &[Row]) {
  println!(
    "{:<8} {:>4}  {:<20} {:>8} {:>12} {:>12}",
    "op", "len", "query", "matches", "scan", "hist"
  );
  for (s, h) in scan.iter().zip(hist) {
    assert_eq!(s.items_count, h.items_count, "baselines disagree");
    println!(
      "{:<8} {:>4}  {:<20} {:>8} {:>9.1} µs {:>9.1} µs",
      s.op,
      s.target.len(),
      s.target,
      s.items_count,
      s.stats.mean_s * 1e6,
      h.stats.mean_s * 1e6,
    );
  }
}

fn main() {
  let args = Args::parse();
  let raw_dict = ensure_raw_dictionary();

  // Parse: read and split for `scan`; also build histograms for `hist`.
  let parse = |op_stats: common::Stats, items_count| Row {
    op: "parse",
    target: "dwyl_english",
    rounds: 5,
    items_count,
    stats: op_stats,
  };
  let (stats, n) = time_rounds(5, || load_words(&read_dict(&raw_dict)).len());
  let mut scan_rows = vec![parse(stats, n)];
  let (stats, n) =
    time_rounds(5, || build_index(&load_words(&read_dict(&raw_dict))).len());
  let mut hist_rows = vec![parse(stats, n)];

  // `query` and `mixed` against the loaded list.
  let text = read_dict(&raw_dict);
  let words = load_words(&text);
  let index = build_index(&words);
  scan_rows.extend(common::run_hot(|rack| {
    sub_anagrams_scan(&words, rack).len()
  }));
  hist_rows.extend(common::run_hot(|rack| {
    sub_anagrams_hist(&index, rack).len()
  }));

  // `startup`: reload the list from disk for every call.
  let mut evictor = CacheEvictor::new();
  scan_rows.extend(common::run_startup(&mut evictor, |rack| {
    sub_anagrams_scan(&load_words(&read_dict(&raw_dict)), rack).len()
  }));
  hist_rows.extend(common::run_startup(&mut evictor, |rack| {
    let text = read_dict(&raw_dict);
    sub_anagrams_hist(&build_index(&load_words(&text)), rack).len()
  }));

  println!("{} words loaded", words.len());
  print_table(&scan_rows, &hist_rows);

  if let Some(dir) = args.save {
    write_csv(&dir, "rust_naive_scan.csv", &scan_rows);
    write_csv(&dir, "rust_naive_hist.csv", &hist_rows);
  }
}
