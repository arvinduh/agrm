<div align="center">
  <h1>agrm</h1>
  <p><b>Blazingly fast anagram and sub-anagram solver backed by a zero-deserialization binary database (<code>.agrm</code>).</b></p>
  <p>
    <a href="https://github.com/arvinduh/anagram_solver/releases"><img src="https://img.shields.io/github/v/release/arvinduh/anagram_solver?style=flat-square" alt="Latest Release"></a>
    <img src="https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-blue?style=flat-square" alt="Platform Support">
    <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
  </p>
</div>

---

## Features

- **Microsecond Lookups**: Queries execute in nanoseconds via zero-copy memory
  mapping (`mmap`).
- **Exact & Sub-Anagrams**: Finds exact full-rack matches or all subset words
  (Scrabble rack solver) with $O(1)$ bitmask pruning.
- **Zero Configuration**: Auto-downloads and initializes the standard dictionary
  on first run if no database exists.
- **Constant Memory Ingestion**: Streams remote files to disk and parses local
  text via virtual memory — no out-of-memory crashes on multi-gigabyte lists.
- **Cross-Platform**: Native support for Linux, macOS, and Windows.

---

## Installation

### One-Line Installers

**Linux & macOS:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/arvinduh/anagram_solver/releases/latest/download/anagram-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://github.com/arvinduh/anagram_solver/releases/latest/download/anagram-installer.ps1 | iex
```

### Build from Source

```bash
git clone https://github.com/arvinduh/anagram_solver.git
cd anagram_solver
cargo install --path .
```

---

## CLI Usage

### 1. `agrm solve` (Query Engine)

Solves anagrams for a given letter rack. If no database exists, it automatically
downloads and indexes the default dictionary into temporary storage on the first
run.

```bash
# Exact anagrams (same letters & length)
agrm solve listen
# Output:
# enlist
# listen
# silent

# Sub-anagrams (all valid words formed by subsets of letters)
agrm solve listen --sub

# Filter sub-anagrams by minimum word length (default: 3)
agrm solve listen --sub --min-len 4

# Limit results and output as JSON
agrm solve listen --sub --json --limit 5
# Output: ["enlist","listen","silent","line","nest"]

# Use a custom database path
agrm solve -d /path/to/custom.agrm listen
```

#### `agrm solve` Options

| Option                | Description                                                     | Default                   |
| :-------------------- | :-------------------------------------------------------------- | :------------------------ |
| `<QUERY>`             | Letters or word to solve (_required_).                          | —                         |
| `-s, --sub`           | Find sub-anagrams (subset words) instead of only exact matches. | `false`                   |
| `-m, --min-len <N>`   | Minimum word length when searching for sub-anagrams.            | `3`                       |
| `-n, --limit <COUNT>` | Maximum number of results to display.                           | Unlimited                 |
| `-d, --db <PATH>`     | Path to `.agrm` database.                                       | `$TMP/english_words.agrm` |
| `--json`              | Format output as a JSON array.                                  | Plain text                |
| `--fast`              | Lazy validation (skips CRC32 scan for microsecond startup).     | `true`                    |

---

### 2. `agrm init` (Database Compiler)

Compiles one or more wordlists (local files, `.agrm` databases, or remote URLs)
into an optimized binary `.agrm` file.

```bash
# Build default dictionary (~370k words) to default location
agrm init

# Build from a local wordlist to a custom location
agrm init -o my_words.agrm /usr/share/dict/words

# Merge multiple sources (local file + remote URL) with deduplication
agrm init -o merged.agrm words.txt https://example.com/extra_words.txt --force
```

#### `agrm init` Options

| Option                | Description                                       | Default                    |
| :-------------------- | :------------------------------------------------ | :------------------------- |
| `[SOURCES]...`        | Input paths or URLs.                              | `words_alpha.txt` (GitHub) |
| `-o, --output <PATH>` | Destination `.agrm` database path.                | `$TMP/english_words.agrm`  |
| `-f, --force`         | Overwrite existing database without confirmation. | `false`                    |
| `-q, --quiet`         | Silence progress and summary statistics.          | `false`                    |

---

## Library Usage (Rust API)

Add this repository to your `Cargo.toml`:

```toml
[dependencies]
anagram = { git = "https://github.com/arvinduh/anagram_solver.git" }
```

### Example

```rust
use anagram::{Reader, Writer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Compile words into an in-memory or on-disk database
    let bytes = Writer::compile_words(["listen", "silent", "enlist", "in", "it", "sit", "line"]);
    let db = Reader::from_bytes(bytes)?;

    // 2. Query exact anagrams with zero deserialization
    let exact: Vec<_> = db.anagrams("listen").collect();
    println!("Exact: {:?}", exact); // ["enlist", "listen", "silent"]

    // 3. Query all sub-anagrams (subset words)
    let subsets = db.sub_anagrams("listen", 3);
    println!("Subsets: {:?}", subsets); // ["enlist", "listen", "silent", "line", "sit", ...]

    Ok(())
}
```

---

## How It Works

1. **Alphabetical Letter Sorting**: Every word is indexed by its sorted ASCII
   characters (e.g. `cat` and `act` share key `act`).
2. **Flattened Bitmask Trie**: Sibling nodes are laid out contiguously on disk
   in alphabetical order. A 26-bit bitmask (`u32`) allows $O(1)$ child indexing
   via hardware `popcount`.
3. **Zero Deserialization**: Database files are memory-mapped (`mmap`). Querying
   traverses memory-mapped arrays and returns string slices (`&str`) directly
   pointing to disk pages.

---

## Benchmarks

Two harnesses time the same 10 racks (3 to 18 letters) against the same
370k-word dictionary and write CSVs to `benches/data/`:

- `cargo bench --bench benchmark -- --save`: the `.agrm` trie
  (`rust_trie.csv`).
- `cargo bench --bench naive -- --save`: two in-memory linear-scan baselines
  (`rust_naive_scan.csv`, `rust_naive_hist.csv`).

Each engine runs every rack under four operations:

| `op`      | What it measures                                                  |
| :-------- | :---------------------------------------------------------------- |
| `parse`   | Building the searchable form of the dictionary from the word list |
| `query`   | One rack repeated back to back; CPU-cache-warm best case          |
| `mixed`   | All racks in a shuffled order; racks evict each other's data      |
| `startup` | Open or load from scratch, then one rack, CPU caches flushed      |

Compare every CSV in `benches/data/` (requires
[uv](https://docs.astral.sh/uv/)):

```bash
uv run benches/compare.py                    # summary table
uv run benches/compare.py --save bench.png   # plus a chart
```

Run both harnesses on the same machine before comparing; the committed CSVs come
from one run each.

---

## License

[MIT](LICENSE)
