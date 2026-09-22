//! The `agrm` binary entry point.

use clap::Parser;

fn main() {
  let cli = anagram::cli::Cli::parse();
  if let Err(err) = anagram::cli::run(cli) {
    eprintln!("[ERROR] {err}");
    std::process::exit(1);
  }
}
