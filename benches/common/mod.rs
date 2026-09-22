//! Workloads shared by every benchmark engine (`benchmark.rs`, `naive.rs`).
//!
//! Each engine is timed under three query workloads, written to CSV as
//! separate `op` values so `compare.py` can line them up:
//!
//! - `query`: one rack repeated back to back. Its working set stays in the CPU
//!   cache, so this is each engine's best case.
//! - `mixed`: every rack once per round, in a shuffled order that is identical
//!   across engines. Racks evict each other's data, as in real use.
//! - `startup`: open or load the dictionary from scratch, then answer one rack,
//!   with the CPU caches flushed first. This is a one-shot CLI call with the
//!   file already in the OS page cache.

#![allow(dead_code)]

use std::hint::black_box;
use std::time::Instant;

/// Minimum sub-anagram length used by every workload.
pub const MIN_LEN: usize = 3;

/// Racks shared by every workload and engine, shortest first.
pub const QUERIES: [&str; 10] = [
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

/// Shuffled passes over `QUERIES` in the `mixed` workload.
const MIXED_ROUNDS: u32 = 20;
/// Timed cold starts per rack in the `startup` workload.
const STARTUP_ROUNDS: u32 = 10;
/// Bytes swept to flush the CPU caches; larger than any current L3.
const EVICT_BYTES: usize = 128 * 1024 * 1024;
const CACHE_LINE: usize = 64;

/// Summary statistics over one row's timed rounds, in seconds.
pub struct Stats {
  pub mean_s: f64,
  pub min_s: f64,
  pub max_s: f64,
  pub median_s: f64,
  pub stddev_s: f64,
}

/// One CSV row: a workload, a rack, and its timings.
pub struct Row {
  pub op: &'static str,
  pub target: &'static str,
  pub rounds: u32,
  pub items_count: usize,
  pub stats: Stats,
}

impl Row {
  /// Formats the row in the shared CSV schema, without a trailing newline.
  pub fn to_csv(&self) -> String {
    let s = &self.stats;
    format!(
      "{},{},{},1,{:.9},{:.9},{:.9},{:.9},{:.9}",
      self.op,
      self.target,
      self.rounds,
      s.mean_s,
      s.min_s,
      s.max_s,
      s.median_s,
      s.stddev_s
    )
  }
}

/// Header line of the shared CSV schema.
pub const CSV_HEADER: &str =
  "op,target,rounds,iterations,mean_s,min_s,max_s,median_s,stddev_s";

/// Computes summary statistics over raw timings in seconds.
pub fn compute_stats(mut times: Vec<f64>) -> Stats {
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

/// Timed rounds for the `query` workload, fewer for slower long racks.
pub fn rounds_for(len: usize) -> u32 {
  if len <= 7 {
    50
  } else if len <= 10 {
    20
  } else {
    10
  }
}

/// Runs `f` once to warm up, then `rounds` timed times.
pub fn time_rounds<T>(rounds: u32, mut f: impl FnMut() -> T) -> (Stats, T) {
  let mut last = f();
  let mut times = Vec::with_capacity(rounds as usize);
  for _ in 0..rounds {
    let t0 = Instant::now();
    last = black_box(f());
    times.push(t0.elapsed().as_secs_f64());
  }
  (compute_stats(times), last)
}

/// Flushes the CPU caches by writing one byte per line of a large buffer.
pub struct CacheEvictor {
  buf: Vec<u8>,
}

impl Default for CacheEvictor {
  fn default() -> Self {
    Self::new()
  }
}

impl CacheEvictor {
  pub fn new() -> Self {
    Self {
      buf: vec![0; EVICT_BYTES],
    }
  }

  pub fn evict(&mut self) {
    for i in (0..self.buf.len()).step_by(CACHE_LINE) {
      self.buf[i] = self.buf[i].wrapping_add(1);
    }
    black_box(&self.buf);
  }
}

/// Deterministic Fisher-Yates shuffle of `QUERIES` indices for `round`.
fn shuffled_order(round: u32) -> [usize; QUERIES.len()] {
  let mut order: [usize; QUERIES.len()] = std::array::from_fn(|i| i);
  // xorshift64, seeded per round so every engine sees the same sequence.
  let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ u64::from(round + 1);
  for i in (1..order.len()).rev() {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    order.swap(i, (state % (i as u64 + 1)) as usize);
  }
  order
}

/// Times `query` under the `query` and `mixed` workloads.
///
/// `query` answers one rack against an already-loaded dictionary and returns
/// the number of words found.
pub fn run_hot<F>(mut query: F) -> Vec<Row>
where
  F: FnMut(&str) -> usize,
{
  let mut rows = Vec::new();

  for target in QUERIES {
    let rounds = rounds_for(target.len());
    let (stats, items_count) = time_rounds(rounds, || query(target));
    rows.push(Row {
      op: "query",
      target,
      rounds,
      items_count,
      stats,
    });
  }

  let mut times =
    vec![Vec::with_capacity(MIXED_ROUNDS as usize); QUERIES.len()];
  let mut counts = [0usize; QUERIES.len()];
  for target in QUERIES {
    black_box(query(target));
  }
  for round in 0..MIXED_ROUNDS {
    for i in shuffled_order(round) {
      let t0 = Instant::now();
      counts[i] = black_box(query(QUERIES[i]));
      times[i].push(t0.elapsed().as_secs_f64());
    }
  }
  for (i, target) in QUERIES.into_iter().enumerate() {
    rows.push(Row {
      op: "mixed",
      target,
      rounds: MIXED_ROUNDS,
      items_count: counts[i],
      stats: compute_stats(std::mem::take(&mut times[i])),
    });
  }

  rows
}

/// Times `startup` under the `startup` workload.
///
/// `startup` opens or loads the dictionary from scratch, answers one rack, and
/// returns the number of words found. CPU caches are flushed before each
/// timed call.
pub fn run_startup<F>(evictor: &mut CacheEvictor, mut startup: F) -> Vec<Row>
where
  F: FnMut(&str) -> usize,
{
  QUERIES
    .into_iter()
    .map(|target| {
      black_box(startup(target));
      let mut times = Vec::with_capacity(STARTUP_ROUNDS as usize);
      let mut items_count = 0;
      for _ in 0..STARTUP_ROUNDS {
        evictor.evict();
        let t0 = Instant::now();
        items_count = black_box(startup(target));
        times.push(t0.elapsed().as_secs_f64());
      }
      Row {
        op: "startup",
        target,
        rounds: STARTUP_ROUNDS,
        items_count,
        stats: compute_stats(times),
      }
    })
    .collect()
}
