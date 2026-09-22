#!/usr/bin/env python3
"""Compares benchmark CSVs with unified tables, empirical scaling analysis, and charts.

This script reads benchmark CSV files, validates their schema, and generates a
summary table of mean latencies and speedups for different engines. It also
produces a line chart visualizing the mean latencies across various operations
and targets.

Usage:
  ```python
  python compare.py [CSV_FILE.csv | DIRECTORY]...
  python compare.py --plot
  python compare.py --save output.png
  ```
"""

from collections.abc import Sequence
from pathlib import Path

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns
from absl import app, flags
from matplotlib import figure

FLAGS = flags.FLAGS
PLOT = flags.DEFINE_bool(
  "plot",
  short_name="p",
  default=False,
  help="Display a temporary Seaborn line chart without saving to repo.",
)
SAVE = flags.DEFINE_string(
  "save",
  default=None,
  help="Optional persistent path to save plot image.",
)


DATA_DIR = Path("./benches/data")
OPS = ("parse", "query", "mixed", "startup")
OP_TITLES = {"query": "Hot Query", "mixed": "Mixed Query", "startup": "Startup"}
REQUIRED_COLUMNS = frozenset({"op", "target", "mean_s"})


def get_csv_files(
  args: Sequence[str], fallback_dir: Path = DATA_DIR
) -> list[str]:
  """Extracts CSV file paths from command-line arguments.

  Arguments can be individual CSV files or directories containing CSVs. If no
  arguments are provided, defaults to the fallback directory. Note that `args`
  should not include the script name (i.e., `sys.argv[1:]`).

  Args:
    args (Sequence[str]): List of command-line arguments (file paths or directories).
    fallback_dir (Path): Directory to search for CSVs if no arguments are provided.

  Returns:
    A sorted list of CSV file paths.

  Raises:
    ValueError: If any provided argument is neither a valid CSV file nor a
      directory.
  """
  if len(args) == 0:
    return sorted(fallback_dir.glob("*.csv"))

  # Use provided arguments as file paths or directories
  csv_files = []
  for file_name in args:
    p = Path(file_name)
    if p.is_file() and file_name.endswith(".csv"):
      csv_files.append(p)
    elif p.is_dir():
      csv_files.extend(sorted(p.glob("*.csv")))
    else:
      raise ValueError(f"Invalid data path: {file_name}")
  return sorted(csv_files)


def load_bench(path: Path) -> pd.DataFrame:
  """Loads benchmark CSV, validating schema and extracting metrics.

  The CSV must contain the required columns: `op`, `target`, and `mean_s`. The
  function will raise a `ValueError` if any required columns are missing. The
  final DataFrame will include the following columns:
  * `op`: The operation type (e.g., 'query', 'parse').
  * `target`: The target word or input for the operation.
  * `length`: The length of the target word.
  * `engine`: The name of the engine (derived from the CSV file name).
  * `mean_ms`: The mean execution time in milliseconds.

  Args:
    path (Path): Path to the CSV file.

  Returns:
    A pandas DataFrame with the benchmark data.

  Raises:
    ValueError: If the CSV file does not match the required schema.
  """
  df = pd.read_csv(path)
  missing = REQUIRED_COLUMNS - set(df.columns)
  if missing:
    raise ValueError(
      f"'{path}' does not match required schema. Missing columns: {sorted(missing)}"
    )

  df["op"] = df["op"].astype(str)
  df["target"] = df["target"].astype(str)
  df["engine"] = path.stem.replace("_", "-")
  df["mean_ms"] = df["mean_s"].astype(float) * 1000.0
  df["length"] = df["target"].str.len()

  return df[["op", "target", "length", "engine", "mean_ms"]]


def format_ms(value: float) -> str:
  """Formats a latency in milliseconds with a readable unit (ns, µs, ms, s)."""
  if pd.isna(value):
    return "-"
  for scale, unit in ((1e3, "s"), (1.0, "ms"), (1e-3, "µs")):
    if value >= scale:
      return f"{value / scale:.2f} {unit}"
  return f"{value * 1e6:.0f} ns"


def sort_engines(df: pd.DataFrame) -> list[str]:
  """Sorts engines by their mean latency across all operations and targets.

  The function computes the mean latency for each engine across all operations
  and targets, then sorts the engines from slowest to fastest. Must be called
  with a DataFrame containing the required columns: `op`, `target`, `length`,
  `engine`, and `mean_ms`.

  Args:
    df (pd.DataFrame): A pandas DataFrame containing benchmark data.

  Returns:
    A pandas Series with engine names as the index and their mean latencies as
    values, sorted in descending order (slowest to fastest).

  """
  return (
    df.groupby(["engine", "op", "target"])["mean_ms"]
    .mean()
    .groupby(level="engine")
    .mean()
    .sort_values(ascending=False)
    .index.tolist()
  )


def generate_summary_table(
  df: pd.DataFrame, engines: list[str]
) -> pd.DataFrame:
  """Generates a summary table of mean latencies and speedups.

  The dataframe MUST contain the required columns: `op`, `target`, `length`,
  `engine`, and `mean_ms`. The summary table will include the mean latencies for
  each engine and, per row, the speedup of the fastest engine over the slowest
  engine that reported that row. Rows are grouped by operation in `OPS` order.

  Args:
    df (pd.DataFrame): A pandas DataFrame containing benchmark data.
    engines (list[str]): Engine names, used as the latency columns.

  Returns:
    A pandas DataFrame with the summary table, including mean latencies for each
    engine and the per-row speedup.
  """
  pivot = df.pivot(
    index=["op", "length", "target"],
    columns="engine",
    values="mean_ms",
  ).reset_index()

  pivot["op"] = pd.Categorical(pivot["op"], categories=OPS, ordered=True)
  pivot.sort_values(
    by=["op", "length", "target"],
    inplace=True,
  )

  pivot.drop(columns=["length"], inplace=True)
  timings = pivot[engines]
  pivot["speedup"] = timings.max(axis=1) / timings.min(axis=1)

  return pivot[["op", "target", *engines, "speedup"]]


def generate_graph(df: pd.DataFrame, engines: list[str]) -> figure.Figure:
  """Generates and returns comparative visualization for every operation.

  One line plot per query workload (`query`, `mixed`, `startup`) shows latency
  by query word, sorted by word length, on a log scale so engines that differ
  by orders of magnitude stay readable. The last subplot is a bar chart of
  catalog ingestion (parse) latencies. Colors are consistent across subplots.

  Args:
    df (pd.DataFrame): Benchmark data containing `op`, `target`, `length`,
    `engine`, and `mean_ms` columns.
    engines (list[str]): List of engine names sorted from slowest to fastest.

  Returns:
    The generated Matplotlib figure.
  """
  # create figure
  sns.set_theme(style="whitegrid", font_scale=0.95)
  query_ops = [op for op in OPS if op != "parse" and (df["op"] == op).any()]
  fig, axes = plt.subplots(
    1,
    len(query_ops) + 1,
    figsize=(6 * len(query_ops) + 4, 4.8),
    gridspec_kw={"width_ratios": [2] * len(query_ops) + [1]},
  )
  *line_axes, ax_bar = axes

  # Query subplots
  for op, ax in zip(query_ops, line_axes):
    sns.lineplot(
      data=df[df["op"] == op].sort_values("length"),
      x="target",
      y="mean_ms",
      hue="engine",
      hue_order=engines,
      palette="tab10",
      linewidth=2.2,
      marker="o",
      ax=ax,
    )
    ax.set_yscale("log")
    ax.set_title(f"{OP_TITLES[op]} Latency by Word", fontweight="bold")
    ax.set_xlabel("Query Word")
    ax.set_ylabel("Avg Latency (ms, log)")
    ax.tick_params(axis="x", rotation=35)
    if ax is not line_axes[0] and ax.get_legend() is not None:
      ax.get_legend().remove()

  # Parse subplot
  sns.barplot(
    data=df[df["op"] == "parse"],
    x="target",
    y="mean_ms",
    hue="engine",
    hue_order=engines,
    palette="tab10",
    width=0.35,  # thin bars
    ax=ax_bar,
  )

  ax_bar.set_title("Parse Latency by Word Bank", fontweight="bold")
  ax_bar.set_xlabel("Word Bank")
  ax_bar.set_ylabel("Avg Latency (ms)")

  # Add legend
  if ax_bar.get_legend() is not None:
    ax_bar.get_legend().remove()
  fig.tight_layout()

  # Save figure to file if requested
  return fig


def main(argv: Sequence[str]) -> None:
  """Main function to compare benchmark CSVs and generate summary and plots.

  Args:
    argv (Sequence[str]): Command-line arguments (excluding the script name).

  Raises:
    ValueError: If no benchmark CSVs are found in the specified data directory.
  """
  # Check inputs
  csv_files = get_csv_files(argv[1:])
  if not csv_files:
    raise ValueError("No benchmark CSVs found in '%s'.", DATA_DIR)

  # Load and combine all benchmark data
  all_data = pd.concat([load_bench(f) for f in csv_files], ignore_index=True)

  sorted_engines = sort_engines(all_data)

  # Generate summary table
  summary = generate_summary_table(all_data, sorted_engines)
  for engine in sorted_engines:
    summary[engine] = summary[engine].map(format_ms)
  summary["speedup"] = summary["speedup"].map(
    lambda value: f"{value:.1f}x" if pd.notna(value) else "-"
  )
  print(summary.to_string(index=False, col_space=14))

  # Generate figure
  if FLAGS.plot or FLAGS.save:
    fig = generate_graph(all_data, sorted_engines)

    # Show figure
    if FLAGS.plot:
      plt.show()

    # Save figure
    if FLAGS.save:
      fig.savefig(FLAGS.save, dpi=200)

    # close figure
    plt.close(fig)


if __name__ == "__main__":
  try:
    app.run(main)
  except Exception as e:
    print(f"Error: {e}")
    exit(1)
