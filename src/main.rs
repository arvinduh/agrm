//! The `agrm` binary entry point.

use clap::Parser;

fn main() {
  let args = anagram::cli::Args::parse();
  if let Err(err) = anagram::cli::run(args) {
    eprintln!("[ERROR] {err}");
    std::process::exit(1);
  }
}
