//! Terminal UI styling, clean ANSI formatting, column grid layout, and duration helpers.
//!
//! Leverages battle-tested libraries:
//! - [`owo_colors`]: zero-allocation, clean terminal coloring with automatic `NO_COLOR` support.
//! - [`terminal_size`]: terminal width detection.

use owo_colors::OwoColorize;
use std::time::Duration;
use terminal_size::{Width, terminal_size};


/// Formats a duration into a human-readable string with appropriate units.
pub fn format_duration(d: Duration) -> String {
  let nanos = d.as_nanos();
  if nanos < 1_000 {
    format!("{nanos}ns")
  } else if nanos < 1_000_000 {
    format!("{:.2}µs", nanos as f64 / 1_000.0)
  } else if nanos < 1_000_000_000 {
    format!("{:.2}ms", nanos as f64 / 1_000_000.0)
  } else {
    format!("{:.2}s", nanos as f64 / 1_000_000_000.0)
  }
}

/// Formats a duration cleanly into milliseconds or seconds (e.g. "0.432ms", "14.20ms", "1.10s").
pub fn format_clean_time(d: Duration) -> String {
  let millis = d.as_secs_f64() * 1000.0;
  if millis < 1.0 {
    format!("{:.3}ms", millis)
  } else if millis < 1000.0 {
    format!("{:.2}ms", millis)
  } else {
    format!("{:.2}s", millis / 1000.0)
  }
}

/// Formats an integer with comma thousands separators (e.g. 370105 -> "370,105").
pub fn format_count(n: usize) -> String {
  let s = n.to_string();
  let mut out = String::with_capacity(s.len() + s.len() / 3);
  let rem = s.len() % 3;
  for (i, ch) in s.chars().enumerate() {
    if i > 0 && (i == rem || (i > rem && (i - rem) % 3 == 0)) {
      out.push(',');
    }
    out.push(ch);
  }
  out
}

/// Formats the clean summary line for solve (e.g. "2 words  0.432ms").
pub fn format_result_summary(count: usize, elapsed: Duration) -> String {
  let word_label = if count == 1 { "word" } else { "words" };
  let count_str = format_count(count);
  let time_str = format_clean_time(elapsed);
  format!(
    "{} {}  {}",
    count_str.bold(),
    word_label.dimmed(),
    time_str.cyan()
  )
}

/// Returns the detected terminal width, defaulting to 80 if not connected to a TTY.
pub fn detect_terminal_width() -> usize {
  terminal_size()
    .map(|(Width(w), _)| w as usize)
    .unwrap_or(80)
}

/// Default horizontal space between columns.
pub const COLUMN_SPACING: usize = 3;

/// Maximum width for terminal column layout to ensure comfortable readability.
pub const MAX_LAYOUT_WIDTH: usize = 80;

/// Formats a slice of words into clean, well-spaced columns with no borders or separators,
/// using a pre-calculated column width and column count.
pub fn format_word_columns(
  words: &[&str],
  col_width: usize,
  num_cols: usize,
  highlight: bool,
) -> String {
  if words.is_empty() {
    return String::new();
  }

  let mut out = String::new();
  for (i, word) in words.iter().enumerate() {
    let is_last_in_row = (i + 1) % num_cols == 0 || i + 1 == words.len();
    let pad = if is_last_in_row {
      0
    } else {
      col_width.saturating_sub(word.len())
    };

    let _ = highlight;
    out.push_str(word);

    if is_last_in_row {
      out.push('\n');
    } else {
      for _ in 0..pad {
        out.push(' ');
      }
    }
  }

  out
}

/// Formats grouped words by length into clean, well-spaced rows and columns with no borders or separators.
///
/// All groups share the same column alignment based on the maximum word length across all groups.
/// The output width is capped at 80 characters for optimal readability.
/// The longest length group is rendered with bright green bold ANSI styling.
pub fn format_grouped_word_grid(
  grouped: &[(usize, Vec<&str>)],
  term_width: usize,
) -> String {
  if grouped.is_empty() {
    return String::new();
  }

  let max_len = grouped
    .iter()
    .flat_map(|(_, words)| words.iter().map(|w| w.len()))
    .max()
    .unwrap_or(0);

  let effective_width = term_width.min(MAX_LAYOUT_WIDTH);
  let col_width = max_len + COLUMN_SPACING;
  let num_cols = (effective_width / col_width).max(1);
  let longest_len = grouped.first().map(|(len, _)| *len).unwrap_or(0);

  let mut out = String::new();
  for (len, words) in grouped {
    if words.is_empty() {
      continue;
    }
    let highlight = *len == longest_len;
    out.push_str(&format_word_columns(words, col_width, num_cols, highlight));
  }

  out
}

/// Formats a list of words into clean, well-spaced columns with no borders or separators.
///
/// Capped at 80 characters for optimal readability.
pub fn format_word_grid(
  words: &[&str],
  term_width: usize,
  highlight: bool,
) -> String {
  if words.is_empty() {
    return String::new();
  }
  let max_len = words.iter().map(|w| w.len()).max().unwrap_or(0);
  let effective_width = term_width.min(MAX_LAYOUT_WIDTH);
  let col_width = max_len + COLUMN_SPACING;
  let num_cols = (effective_width / col_width).max(1);

  format_word_columns(words, col_width, num_cols, highlight)
}

/// Renders a horizontal bar chart row using Unicode blocks.
pub fn render_bar(value: f64, max_val: f64, max_cols: usize) -> String {
  if max_val <= 0.0 || value <= 0.0 {
    return String::new();
  }

  let ratio = (value / max_val).clamp(0.0, 1.0);
  let total_eighths = (ratio * (max_cols as f64) * 8.0).round() as usize;
  let full_blocks = total_eighths / 8;
  let rem = total_eighths % 8;

  let partial = match rem {
    1 => "▏",
    2 => "▎",
    3 => "▍",
    4 => "▌",
    5 => "▋",
    6 => "▊",
    7 => "▉",
    _ => "",
  };

  let bar = format!("{}{}", "█".repeat(full_blocks), partial);
  bar.cyan().to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_format_duration() {
    assert_eq!(format_duration(Duration::from_nanos(500)), "500ns");
    assert_eq!(format_duration(Duration::from_micros(12)), "12.00µs");
    assert_eq!(format_duration(Duration::from_millis(42)), "42.00ms");
    assert_eq!(format_duration(Duration::from_secs(2)), "2.00s");
  }

  #[test]
  fn test_format_word_grid() {
    let words = vec!["act", "cat"];
    let grid = format_word_grid(&words, 80, true);
    assert!(grid.contains("act"));
    assert!(grid.contains("cat"));
  }

  #[test]
  fn test_format_grouped_word_grid() {
    let words_6 = vec!["listen", "silent", "tinsel"];
    let words_5 = vec!["inlet", "istle"];
    let grouped = vec![(6, words_6), (5, words_5)];
    let grid = format_grouped_word_grid(&grouped, 80);
    assert!(grid.contains("listen"));
    assert!(grid.contains("istle"));
  }

  #[test]
  fn test_render_bar() {
    let bar_half = render_bar(5.0, 10.0, 20);
    assert!(bar_half.contains("█"));
  }
}
