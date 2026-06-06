use serde::Deserialize;

/// Program configuration at start.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Unit system applied to all dimensional values (e.g. `stock_size`, `tool_length`).
    pub units: Unit,
    /// Size of the stock along each axis.
    /// Each axis value is guaranteed to be positive and non zero.
    pub stock_size: Point,
    /// Work offset zero position, relative to stock dimensions.
    pub zero_pos: ZeroPosition,
    /// Collection of tool configurations to be used during G-code execution.
    pub tools: Vec<ToolConfig>,
}

/// Possible unit standards for dimensional values.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Metric,
    Imperial,
}

/// A 3D point in space.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Point {
    x: f32,
    y: f32,
    z: f32,
}

impl PartialEq for Point {
    fn eq(&self, other: &Self) -> bool {
        self.x - other.x < 1e-10 && self.y - other.y < 1e-10 && self.z - other.z < 1e-10
    }
}

/// Tool configuration.
#[derive(Debug, Deserialize, PartialEq)]
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
#[derive(Debug, Deserialize, PartialEq)]
pub struct ZeroPosition {
    pub x: AxisPoint,
    pub y: AxisPoint,
    pub z: AxisPoint,
}

/// Possible zero position on each axis of the stock.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AxisPoint {
    Zero,
    Mid,
    Max,
}

impl Config {
    /// Constructs a config by attempting to parse a provided string slice.
    ///
    /// # Errors:
    /// - [`ConfigError::Parse`] -- Could not parse the provided slice.
    /// - [`ConfigError::StockNonPositive`] -- At least one of the stock axis was zero or negative.
    /// - [`ConfigError::ToolNonPositive`] -- At least one of the tools has zero or negative
    ///   diameter or length.
    pub fn build(json: &str) -> Result<Self, ConfigError> {
        let mut ret: Self = serde_json::from_str(json)?;

        // make sure stock size and tool diameter and length are positive and non zero
        if ret.stock_size.x < 1e-5 {
            Err(ConfigError::StockNonPositive('X'))
        } else if ret.stock_size.y < 1e-5 {
            Err(ConfigError::StockNonPositive('Y'))
        } else if ret.stock_size.z < 1e-5 {
            Err(ConfigError::StockNonPositive('Z'))
        } else {
            for tool in &mut ret.tools {
                if tool.diameter < 1e-5 || tool.length < 1e-5 {
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
    #[error("failed to parse JSON")]
    Parse(#[from] serde_json::Error),
    #[error("stock dimension is either negative or zero for '{}' axis", .0)]
    StockNonPositive(char),
    #[error("diameter or length is either negative or zero for tool number '{}'", .0)]
    ToolNonPositive(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let ret = Config::build(json).unwrap();

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
              \"tools\": [
                {
                  \"number\": 1,
                  \"diameter\": 5,
                  \"length\": 10
                }
              ]
            }";

        Config::build(json).unwrap();
    }

    #[test]
    #[should_panic = "unknown field `excess`, expected one of `units`, `stock_size`, `zero_pos`, `tools`"]
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
              \"tools\": [],
              \"excess\": \"invalid\"
            }";

        Config::build(json).unwrap();
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
              \"tools\": []
            }";

        Config::build(json).unwrap_or_else(|e| panic!("{e}"));
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
              \"tools\": [
                {
                  \"number\": 2,
                  \"diameter\": -5,
                  \"length\": 10
                }
              ]
            }";

        Config::build(json).unwrap_or_else(|e| panic!("{e}"));
    }
}
