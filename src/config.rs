//! # GSim Configuration
//!
//! Command line arguments parser.
//! Extracts the first argument as **G-Code source** file path,
//! with optional machine axis travels.

use crate::parser::Point;
use clap::Parser;

/// Command line arguments.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Path of the input G-code file. If not provided, stdin is targeted instead.
    pub filepath: Option<String>,
    /// Maximum travel of the machine in X axis.
    #[arg(short, default_value_t = 500)]
    x: u32,
    /// Maximum travel of the machine in Y axis.
    #[arg(short, default_value_t = 250)]
    y: u32,
    /// Maximum travel of the machine in Z axis.
    #[arg(short, default_value_t = 250)]
    z: u32,
}

impl Config {
    /// Constructs [`Point`] with maximum axis travels provided
    /// either by the user or set by the defaults.
    pub fn max_travels(&self) -> Point {
        Point::new(self.x as f64, self.y as f64, self.z as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify() {
        Config::command().debug_assert();
    }
}
