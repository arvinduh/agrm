//! # Anagram
//!
//! `anagram` is a high-performance anagram solver backed by a compact,
//! zero-deserialization binary word database (`.agrm`).
//!
//! ## Architecture & Format
//!
//! A word list is compiled into a flattened trie where each node is keyed by the
//! sorted ASCII letters of its words. As a result, all anagrams of a given letter
//! set share a single trie node, and their raw text is packed into a contiguous
//! slice within the word pool.
//!
//! Database files are loaded into memory and queried with zero parsing overhead.
//!
//! ## Example
//!
//! ```rust
//! use std::io::Cursor;
//! use anagram::{Reader, Writer};
//!
//! // 1. Compile words into a buffer or file.
//! let mut buf = Cursor::new(Vec::new());
//! Writer::new(&mut buf).write_words(["act", "cat", "dog", "god"])?;
//!
//! // 2. Query anagrams with zero parsing overhead.
//! let db = Reader::from_bytes(buf.into_inner())?;
//! let mut hits: Vec<_> = db.anagrams("tac").collect();
//! hits.sort_unstable();
//! assert_eq!(hits, ["act", "cat"]);
//! # Ok::<(), anagram::error::Error>(())
//! ```

pub mod cli;
pub mod error;
pub mod format;
pub mod ingest;
pub mod ui;

/// Engine-wide constants and architectural bounds.
pub mod constants;
/// Core data structures for words and in-memory tries.
pub mod core;

pub use core::trie::{Trie, TrieNode};
pub use core::word::Word;
pub use error::{Error, Result};
pub use format::reader::Reader;
pub use format::writer::Writer;
pub use ingest::{DEFAULT_DICTIONARY_URL, Source, default_cache_path, ingest};
