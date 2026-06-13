//! # GSim Configuration
//!
//! Configuration file parser.
//! Reads and parses **JSON** config file at a provided file path.
//!
//! ## Example
//! The following is an example of a valid config file:
//! ```text
//! {
//!  "units": "metric",
//!  "stock_size": {
//!    "x": 500,
//!    "y": 500,
//!    "z": 500
//!  },
//!  "zero_pos": {
//!    "x": "mid",
//!    "y": "mid",
//!    "z": "max"
//!  },
//!  "start_pos": {
//!    "x": 0,
//!    "y": 0,
//!    "z": 100
//!  },
//!  "tools": [
//!    {
//!      "number": 1,
//!      "diameter": 5,
//!      "length": 10
//!    },
//!    {
//!      "number": 2,
//!      "diameter": 5,
//!      "length": 10
//!    }
//!  ]
//! }
//! ```
//! - Treats every dimension in `metric` system.
//! - Creates a stock with each side measuring `500mm`.
//! - Anchors the `zero_pos` at middle of **X**(250mm), middle of **Y**(250mm) and top of
//! **Z**(500mm).
//! - Creates two tools(numbered `1` & `2`), each with `diameter` `5mm` and `length` `10mm`.
//!
//! ## Restrictions
//! - Any **excess elements** will be rejected.
//! - `units` can only have two possible values: `imperial` or `metric`.
//! - Every stock dimension **must** be positive and non-zero.
//! - `zero_pos` for each axis can only have three possible values: `zero`, `mid` or `max`.
//! - Each tool `diameter` and `length` **must** be positive and non-zero.

use crate::FLOAT_VARIANCE;
use serde::{Deserialize, Serialize};

/// Program configuration at start.
#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Unit system applied to all dimensional values (e.g. `stock_size`, `tool_length`).
    pub units: Unit,
    /// Size of the stock along each axis.
    /// Each axis value is guaranteed to be positive and non zero.
    pub stock_size: Point,
    /// Work offset zero position, relative to stock dimensions.
    pub zero_pos: ZeroPosition,
    /// Start position at the beginning of the program.
    /// This is relative to [`Self::zero_pos`].
    pub start_pos: Point,
    /// Collection of tool configurations to be used during G-code execution.
    pub tools: Vec<ToolConfig>,
}

/// Possible unit standards for dimensional values.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize,)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Metric,
    Imperial,
}

/// A 3D point in space.
#[derive(Clone, Copy, Debug, Deserialize, Serialize,)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl PartialEq for Point {
    fn eq(&self, other: &Self) -> bool {
        (self.x - other.x).abs() < FLOAT_VARIANCE
            && (self.y - other.y).abs() < FLOAT_VARIANCE
            && (self.z - other.z).abs() < FLOAT_VARIANCE
    }
}

/// Tool configuration.
#[derive(Debug, Deserialize, PartialEq, Serialize,)]
pub struct ToolConfig {
    /// Number of the tool.
    /// This is denoted with a `T` code in G-code.
    pub number: u32,
    /// Diameter of the tool to render when `self.number` tool is activated.
    /// This is guaranteed to be positive and non zero.
    pub diameter: f32,
    /// Length of the tool to render when `self.number` tool is activated.
    /// This is guaranteed to be positive and non zero.
    pub length: f32,
}

/// Work offset zero position.
///
/// This is similar to the `G54` work piece offset in G-code,
/// but is always active.
///
/// This can only be at the middle or any of the ends of each axis of the stock.
/// Therefore, giving us 27 total possible combinations.
#[derive(Debug, Deserialize, PartialEq, Serialize,)]
pub struct ZeroPosition {
    pub x: AxisPoint,
    pub y: AxisPoint,
    pub z: AxisPoint,
}

/// Possible zero position on each axis of the stock.
#[derive(Debug, Deserialize, PartialEq, Serialize,)]
#[serde(rename_all = "lowercase")]
pub enum AxisPoint {
    Zero,
    Mid,
    Max,
}

impl Config {
    /// Constructs a config by attempting to read a JSON file and then parse it.
    ///
    /// For additional information checkout [`Self::from_str`].
    ///
    /// # Errors:
    /// - [`ConfigError::IO`] -- Could not read the file at provided path.
    /// - [`ConfigError::Parse`] -- Could not parse the provided slice.
    /// - [`ConfigError::StockNonPositive`] -- At least one of the stock axis was zero or negative.
    /// - [`ConfigError::ToolNonPositive`] -- At least one of the tools has zero or negative
    ///   diameter or length.
    pub fn from_file(path: &str) -> Result<Self, ConfigError> {
        Self::from_str(
            std::fs::read_to_string(path)
                .map_err(|e| ConfigError::IO(e, path.to_owned()))?
                .as_str(),
        )
    }

    /// Constructs a config by attempting to parse a provided string slice.
    ///
    /// # Errors:
    /// - [`ConfigError::Parse`] -- Could not parse the provided slice.
    /// - [`ConfigError::StockNonPositive`] -- At least one of the stock axis was zero or negative.
    /// - [`ConfigError::ToolNonPositive`] -- At least one of the tools has zero or negative
    ///   diameter or length.
    pub fn from_str(json: &str) -> Result<Self, ConfigError> {
        let mut ret: Self = serde_json::from_str(json)?;

        // make sure stock size and tool diameter and length are positive and non zero
        if ret.stock_size.x < FLOAT_VARIANCE {
            Err(ConfigError::StockNonPositive('X'))
        } else if ret.stock_size.y < FLOAT_VARIANCE {
            Err(ConfigError::StockNonPositive('Y'))
        } else if ret.stock_size.z < FLOAT_VARIANCE {
            Err(ConfigError::StockNonPositive('Z'))
        } else {
            for tool in &mut ret.tools {
                if tool.diameter < FLOAT_VARIANCE || tool.length < FLOAT_VARIANCE {
                    return Err(ConfigError::ToolNonPositive(tool.number));
                }
            }

            Ok(ret)
        }
    }

    /// Converts a [`Self::zero_pos`] to a [`Point`] filled with the absolute positive of the
    /// `zero_pos`.
    pub fn zero_point(&self) -> Point {
        Point {
            x: match self.zero_pos.x {
                AxisPoint::Zero => 0.0,
                AxisPoint::Mid => self.stock_size.x / 2.0,
                AxisPoint::Max => self.stock_size.x,
            },
            y: match self.zero_pos.y {
                AxisPoint::Zero => 0.0,
                AxisPoint::Mid => self.stock_size.y / 2.0,
                AxisPoint::Max => self.stock_size.y,
            },
            z: match self.zero_pos.z {
                AxisPoint::Zero => 0.0,
                AxisPoint::Mid => self.stock_size.z / 2.0,
                AxisPoint::Max => self.stock_size.z,
            },
        }
    }
}

/// Possible errors that can happen during [`Config`] construction.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read the config file.
    #[error("failed to read file at '{}'", .1)]
    IO(#[source] std::io::Error, String),
    /// Failed to parse the config file as JSON.
    #[error("failed to parse JSON")]
    Parse(#[from] serde_json::Error),
    /// Stock dimensions are not all positive.
    #[error("stock dimension is either negative or zero for '{}' axis", .0)]
    StockNonPositive(char),
    /// Tool dimensions are not all positive.
    #[error("diameter or length is either negative or zero for tool number '{}'", .0)]
    ToolNonPositive(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic = "failed to read file at 'notfound'"]
    fn file() {
        Config::from_file("notfound").unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn good() {
        let json = "
            {
              \"units\": \"metric\",
              \"stock_size\": {
                \"x\": 500,
                \"y\": 500,
                \"z\": 500
              },
              \"zero_pos\": {
                \"x\": \"mid\",
                \"y\": \"mid\",
                \"z\": \"max\"
              },
              \"start_pos\" : {
                \"x\": 0,
                \"y\": 0,
                \"z\": 100
              },
              \"tools\": [
                {
                  \"number\": 1,
                  \"diameter\": 5,
                  \"length\": 10
                },
                {
                  \"number\": 2,
                  \"diameter\": 5,
                  \"length\": 10
                }
              ]
            }";

        let ret = Config::from_str(json).unwrap();

        // stock and tool sizes will always be positive regardless of the sign
        assert_eq!(
            ret,
            Config {
                units: Unit::Metric,
                stock_size: Point {
                    x: 500.0,
                    y: 500.0,
                    z: 500.0
                },
                zero_pos: ZeroPosition {
                    x: AxisPoint::Mid,
                    y: AxisPoint::Mid,
                    z: AxisPoint::Max
                },
                start_pos: Point {
                    x: 0.0,
                    y: 0.0,
                    z: 100.0
                },
                tools: vec![
                    ToolConfig {
                        number: 1,
                        diameter: 5.0,
                        length: 10.0,
                    },
                    ToolConfig {
                        number: 2,
                        diameter: 5.0,
                        length: 10.0,
                    }
                ]
            }
        );

        assert_eq!(
            ret.zero_point(),
            Point {
                x: 250.0,
                y: 250.0,
                z: 500.0
            }
        );
    }

    #[test]
    #[should_panic = "unknown variant `invalid`, expected `metric` or `imperial`"]
    fn bad_units() {
        let json = "
            {
              \"units\": \"invalid\",
              \"stock_size\": {
                \"x\": 500,
                \"y\": 500,
                \"z\": 500
              },
              \"zero_pos\": {
                \"x\": \"mid\",
                \"y\": \"mid\",
                \"z\": \"max\"
              },
              \"start_pos\" : {
                \"x\": 0,
                \"y\": 0,
                \"z\": 100
              },
              \"tools\": [
                {
                  \"number\": 1,
                  \"diameter\": 5,
                  \"length\": 10
                }
              ]
            }";

        Config::from_str(json).unwrap();
    }

    #[test]
    #[should_panic = "unknown field `excess`, expected one of `units`, `stock_size`, `zero_pos`, `start_pos`, `tools`"]
    fn excess() {
        let json = "
            {
              \"units\": \"metric\",
              \"stock_size\": {
                \"x\": 500,
                \"y\": 500,
                \"z\": 500
              },
              \"zero_pos\": {
                \"x\": \"mid\",
                \"y\": \"mid\",
                \"z\": \"max\"
              },
              \"start_pos\" : {
                \"x\": 0,
                \"y\": 0,
                \"z\": 100
              },
              \"tools\": [],
              \"excess\": \"invalid\"
            }";

        Config::from_str(json).unwrap();
    }

    #[test]
    #[should_panic = "stock dimension is either negative or zero for 'X' axis"]
    fn invalid_stock() {
        let json = "
            {
              \"units\": \"metric\",
              \"stock_size\": {
                \"x\": -500,
                \"y\": 500,
                \"z\": 500
              },
              \"zero_pos\": {
                \"x\": \"mid\",
                \"y\": \"mid\",
                \"z\": \"max\"
              },
              \"start_pos\" : {
                \"x\": 0,
                \"y\": 0,
                \"z\": 100
              },
              \"tools\": []
            }";

        Config::from_str(json).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    #[should_panic = "diameter or length is either negative or zero for tool number '2'"]
    fn invalid_tool() {
        let json = "
            {
              \"units\": \"metric\",
              \"stock_size\": {
                \"x\": 500,
                \"y\": 500,
                \"z\": 500
              },
              \"zero_pos\": {
                \"x\": \"mid\",
                \"y\": \"mid\",
                \"z\": \"max\"
              },
              \"start_pos\" : {
                \"x\": 0,
                \"y\": 0,
                \"z\": 100
              },
              \"tools\": [
                {
                  \"number\": 2,
                  \"diameter\": -5,
                  \"length\": 10
                }
              ]
            }";

        Config::from_str(json).unwrap_or_else(|e| panic!("{e}"));
    }
}
