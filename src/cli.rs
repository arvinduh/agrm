//! CLI command definitions, routing, and shared configuration.

pub mod init;
pub mod solve;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// High-performance anagram solver backed by the `.agrm` binary database format.
#[derive(Parser)]
#[command(
  name = "agrm",
  author,
  version,
  about = "High-performance anagram solver backed by the .agrm binary database",
  long_about = None
)]
pub struct Cli {
  /// The command to execute.
  #[command(subcommand)]
  pub command: Commands,
}

/// Available subcommands for `agrm`.
#[derive(Subcommand)]
pub enum Commands {
  /// Compiles dictionary sources into an optimized .agrm database.
  Init(init::InitArgs),

  /// Solves exact anagrams or sub-anagrams for a given set of letters.
  Solve(solve::SolveArgs),
}

/// Resolves the database path using priority: CLI arg > AGRM_DB env var > OS temp path.
pub fn resolve_db_path(explicit: Option<PathBuf>) -> PathBuf {
  if let Some(path) = explicit {
    return path;
  }
  if let Ok(env_path) = std::env::var("AGRM_DB") {
    return PathBuf::from(env_path);
  }
  crate::ingest::default_cache_path()
}

/// Dispatches the parsed command to its respective handler.
pub fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  match cli.command {
    Commands::Init(args) => init::run(args),
    Commands::Solve(args) => solve::run(args),
  }
}
