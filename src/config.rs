//! # GSim Configuration
//!
//! Command line arguments parser.
//! Extracts the first argument as **G-Code source** file path.

use clap::Parser;

/// Command line arguments.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Path of the input G-Code file.
    pub filepath: String,
}
