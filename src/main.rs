//! The `agrm` binary entry point.

use clap::Parser;
use std::process::ExitCode;

use anagram::cli::{self, Args};

fn main() -> ExitCode {
  let args = Args::parse();
  match cli::run(args) {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      eprintln!("[ERROR] {err}");
      ExitCode::FAILURE
    }
  }
}
