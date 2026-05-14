use std::{cmp::Ordering, f64::consts::PI, mem::size_of};

use winit::dpi::PhysicalSize;

use crate::{
    View,
    machine::{Arc, CircularDirection, Line, MotionSummary, PlanarPoint},
    parser::{Plane, Point},
};

const SHOW_MACHINE_BOUNDARY: bool = false;
const SHOW_GRID: bool = true;
const SHOW_ORIGIN: bool = true;

const DEFAULT_STROKE_WIDTH: f32 = 0.004;
const MACHINE_BOUNDARY_WIDTH: f32 = DEFAULT_STROKE_WIDTH * 2.0;
const ORIGIN_WIDTH: f32 = DEFAULT_STROKE_WIDTH * 2.0;
const GRID_WIDTH: f32 = DEFAULT_STROKE_WIDTH * 0.8;

const MACHINE_BOUNDARY_COLOR: [f32; 3] = [0.69, 0.69, 0.69]; // noice
const RAPID_MOVE_COLOR: [f32; 3] = [1.0, 0.05, 0.05];
const FEED_MOVE_COLOR: [f32; 3] = [0.1, 1.0, 0.1];
const GRID_COLOR: [f32; 3] = [0.1, 0.1, 0.1];
const X_AXIS_COLOR: [f32; 3] = [1.0, 0.0, 0.0];
const Y_AXIS_COLOR: [f32; 3] = [0.0, 1.0, 0.0];
const Z_AXIS_COLOR: [f32; 3] = [0.0, 0.0, 1.0];

const TOOL_COLOR: [f32; 4] = [0.25, 0.25, 0.25, 1.0];
// units travelled per frame
const SPEED: f64 = 5.0;

/// Configuration of fixed [`LineInstance`]s that can be toggled.
#[derive(Clone, Copy)]
pub struct StaticConfig {
    /// Whether to render machine travels boundary box.
    machine_boundary: bool,
    /// Whether to render the reference grid on [`Plane::XY`].
    grid: bool,
    /// Whether to render X, Y, and Z axis indicators, rooted at origin.
    origin: bool,
}

impl Default for StaticConfig {
    /// Generates a default config for static [`LineInstances`]s,
    /// based on the compile-time constants.
    fn default() -> Self {
        Self {
            machine_boundary: SHOW_MACHINE_BOUNDARY,
            grid: SHOW_GRID,
            origin: SHOW_ORIGIN,
        }
    }
}

impl StaticConfig {
    /// Toggles machine travel boundary box on or off.
    pub fn toggle_machine_boundary(&mut self) {
        self.machine_boundary = !self.machine_boundary
    }

    /// Toggles the XY plane reference grid on or off.
    pub fn toggle_grid(&mut self) {
        self.grid = !self.grid
    }

    /// Toggles all axis indicators on or off.
    pub fn toggle_origin(&mut self) {
        self.origin = !self.origin
    }
}

/// Represents a straight line between two points,
/// that can be drawn to the screen with a vertex shader.
///
/// The vertex shader creates 6 vertices (two triangles) per line instance,
/// to create a line with variable thickness.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineInstance {
    /// 3D start point of the line.
    start: [f32; 3],
    /// 3D end point of the line.
    pub end: [f32; 3],
    /// RGB color of the line.
    color: [f32; 3],
    /// Width of the rendered line, in pixels.
    stroke_width: f32,
}

impl LineInstance {
    /// Returns a [`VertexBufferLayout`](wgpu::VertexBufferLayout) that describes how
    /// [`LineInstance`]s are stored in a GPU buffer.
    ///
    /// The layout is set to use [`VertexStepMode::Instance`](wgpu::VertexStepMode::Instance),
    /// which allows the vertex shader to expand a single line segment into polygons (two triangles)
    /// by receiving the same [`LineInstance`] 6 times.
    pub fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            // share the same buffer entry across a number of vertices
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[f32; 6]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[f32; 9]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }

    pub fn statics(max_travels: Point, static_config: StaticConfig) -> Vec<Self> {
        let mut ret = vec![];
        let x = max_travels.x() as f32;
        let y = max_travels.y() as f32;
        let z = max_travels.z() as f32;

        let boundary_stroke_width = if static_config.machine_boundary {
            MACHINE_BOUNDARY_WIDTH
        } else {
            0.0
        };
        let grid_stroke_width = if static_config.grid { GRID_WIDTH } else { 0.0 };
        let origin_stroke_width = if static_config.origin {
            ORIGIN_WIDTH
        } else {
            0.0
        };

        ret.extend_from_slice(&[
            Self {
                start: [x, y, z],
                end: [0.0, y, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [x, y, z],
                end: [x, 0.0, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [0.0, y, z],
                end: [0.0, 0.0, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [x, 0.0, z],
                end: [0.0, 0.0, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [x, y, 0.0],
                end: [0.0, y, 0.0],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [x, y, 0.0],
                end: [x, 0.0, 0.0],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [0.0, y, 0.0],
                end: [0.0, 0.0, 0.0],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [x, 0.0, 0.0],
                end: [0.0, 0.0, 0.0],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [x, y, 0.0],
                end: [x, y, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [x, 0.0, 0.0],
                end: [x, 0.0, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [0.0, y, 0.0],
                end: [0.0, y, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
            Self {
                start: [0.0, 0.0, 0.0],
                end: [0.0, 0.0, z],
                color: MACHINE_BOUNDARY_COLOR,
                stroke_width: boundary_stroke_width,
            },
        ]);

        ret.extend_from_slice(&[
            Self {
                start: [0.0, 0.0, 0.0],
                end: [x * 2.0, 0.0, 0.0],
                color: X_AXIS_COLOR,
                stroke_width: origin_stroke_width,
            },
            Self {
                start: [0.0, 0.0, 0.0],
                end: [0.0, y * 2.0, 0.0],
                color: Y_AXIS_COLOR,
                stroke_width: origin_stroke_width,
            },
            Self {
                start: [0.0, 0.0, 0.0],
                end: [0.0, 0.0, z * 2.0],
                color: Z_AXIS_COLOR,
                stroke_width: origin_stroke_width,
            },
        ]);

        let step = if x > 1000.0 {
            100.0
        } else if x > 500.0 {
            50.0
        } else if x > 250.0 {
            25.0
        } else {
            10.0
        };

        let mut current_x = 0.0;
        let mut current_y = 0.0;

        while current_x < x * 2.0 {
            current_x += step;
            ret.push(Self {
                start: [current_x, -y * 2.0, 0.0],
                end: [current_x, y * 2.0, 0.0],
                color: GRID_COLOR,
                stroke_width: grid_stroke_width,
            });
        }

        current_x = 0.0;

        while current_x > -x * 2.0 {
            current_x -= step;
            ret.push(Self {
                start: [current_x, -y * 2.0, 0.0],
                end: [current_x, y * 2.0, 0.0],
                color: GRID_COLOR,
                stroke_width: grid_stroke_width,
            });
        }

        while current_y < y * 2.0 {
            current_y += step;
            ret.push(Self {
                start: [-x * 2.0, current_y, 0.0],
                end: [x * 2.0, current_y, 0.0],
                color: GRID_COLOR,
                stroke_width: grid_stroke_width,
            });
        }

        current_y = 0.0;

        while current_y > -y * 2.0 {
            current_y -= step;
            ret.push(Self {
                start: [-x * 2.0, current_y, 0.0],
                end: [x * 2.0, current_y, 0.0],
                color: GRID_COLOR,
                stroke_width: grid_stroke_width,
            });
        }

        ret
    }

    pub fn rapid_move(start: Point, end: Point) -> Self {
        Self {
            start: [start.x() as f32, start.y() as f32, start.z() as f32],
            end: [end.x() as f32, end.y() as f32, end.z() as f32],
            color: RAPID_MOVE_COLOR,
            stroke_width: DEFAULT_STROKE_WIDTH,
        }
    }

    pub fn feed_move(start: Point, end: Point) -> Self {
        Self {
            start: [start.x() as f32, start.y() as f32, start.z() as f32],
            end: [end.x() as f32, end.y() as f32, end.z() as f32],
            color: FEED_MOVE_COLOR,
            stroke_width: DEFAULT_STROKE_WIDTH,
        }
    }
}

pub enum LineInstances {
    Linear(Box<dyn Iterator<Item = LineInstance>>),
    Arc(Box<dyn Iterator<Item = LineInstance>>),
}

impl LineInstances {
    pub fn new(summary: MotionSummary) -> Self {
        match summary {
            MotionSummary::Rapid(line) => Self::linear_points(line, LineInstance::rapid_move),
            MotionSummary::Feed(line) => Self::linear_points(line, LineInstance::feed_move),
            MotionSummary::Arc(arc) => Self::arc_points(arc),
        }
    }

    // takes in a function pointer that provides the vertex
    fn linear_points(line: Line, vertex: fn(Point, Point) -> LineInstance) -> Self {
        let start = line.start;
        let end = line.end;

        // direction from start to end
        let dir = end - start;
        // distance between start and end points
        let dist = (dir.x().powi(2) + dir.y().powi(2) + dir.z().powi(2)).sqrt();

        if dist <= SPEED {
            return Self::Linear(Box::new([vertex(start, end)].into_iter()));
        }

        // amount to move each axis by to get next point
        let delta = dir.mul_float(SPEED).div_float(dist);

        let mut current = start;

        Self::Linear(Box::new(std::iter::from_fn(move || {
            if current == end {
                return None;
            }

            let next = current + delta;
            let remaining = end - next;

            // use dot product to see if the next point is between start and end
            if remaining.x() * dir.x() + remaining.y() * dir.y() + remaining.z() * dir.z() <= 0.0 {
                current = end;
            } else {
                current = next;
            }

            Some(vertex(start, current))
        })))
    }

    // always drawn in feed
    // reference: https://www.freemathhelp.com/forum/threads/xy-points-on-an-arc.130791/
    fn arc_points(arc: Arc) -> Self {
        let plane = arc.center.plane();
        let start = PlanarPoint::from_point(arc.start, plane);

        let center = arc.center;
        let radius = arc.radius;
        let sweep = arc.sweep;

        // angular speed
        let step_angular = match arc.dir {
            CircularDirection::Clockwise => 0.0 - SPEED / radius,
            CircularDirection::CounterClockwise => SPEED / radius,
        };
        let steps_count = (sweep / step_angular).ceil().abs();
        let step_linear = match plane {
            Plane::XY => arc.end.z() - arc.start.z(),
            Plane::XZ => arc.end.y() - arc.start.y(),
            Plane::YZ => arc.end.x() - arc.start.x(),
        } / steps_count;

        if sweep.abs() <= step_angular.abs() {
            return Self::Arc(Box::new(
                [LineInstance::feed_move(arc.start, arc.end)].into_iter(),
            ));
        }

        // start point relative to arc center
        let rel_start = start - center;

        // minor arc sweep angle with primary axis of the plane in radians
        let mut current_sweep = (rel_start.first() / radius).clamp(-1.0, 1.0).acos();
        if rel_start.second().is_sign_negative() {
            current_sweep += PI;
        }
        let mut current_pos = arc.start;
        // total sweep from positive major axis to get to end point
        let end_sweep = current_sweep + sweep;

        Self::Arc(Box::new(std::iter::from_fn(move || {
            // both are exact same on bit level
            if current_sweep == end_sweep {
                return None;
            }

            current_sweep = match arc.dir {
                CircularDirection::Clockwise => {
                    if current_sweep + step_angular < end_sweep {
                        end_sweep
                    } else {
                        current_sweep + step_angular
                    }
                }
                CircularDirection::CounterClockwise => {
                    if current_sweep + step_angular > end_sweep {
                        end_sweep
                    } else {
                        current_sweep + step_angular
                    }
                }
            };

            // relative to center
            let new_pos = if current_sweep == end_sweep {
                arc.end
            } else {
                match plane {
                    Plane::XY => Point::new(
                        arc.center.first() + radius * current_sweep.cos(),
                        arc.center.second() + radius * current_sweep.sin(),
                        current_pos.z() + step_linear,
                    ),
                    Plane::XZ => Point::new(
                        arc.center.first() + radius * current_sweep.cos(),
                        current_pos.y() + step_linear,
                        arc.center.second() + radius * current_sweep.sin(),
                    ),
                    Plane::YZ => Point::new(
                        current_pos.x() + step_linear,
                        arc.center.first() + radius * current_sweep.cos(),
                        arc.center.second() + radius * current_sweep.sin(),
                    ),
                }
            };

            let ret = Some(LineInstance::feed_move(current_pos, new_pos));

            current_pos = new_pos;

            ret
        })))
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    window_size: [f32; 2],
    // padding in pixels to center machine view inside the window
    padding: [f32; 2],
    // signed max travels for each axis, starting at 0 for each axis
    max_travels: [f32; 4],
    // color of the tool
    tool_color: [f32; 4],
    // diameter of the tool
    tool_size: f32,
    // length of the tool
    tool_len: f32,
    // absolute scale, to convert machine unit to pixels
    scale: f32,
    view: View,
}

impl Uniforms {
    pub fn new(window_size: PhysicalSize<u32>, max_travels: Point) -> Self {
        let window_size = [window_size.width as f32, window_size.height as f32];
        let max_travels = [
            max_travels.x() as f32,
            max_travels.y() as f32,
            max_travels.z() as f32,
            0.0,
        ];

        let machine_size = machine_size(max_travels.as_slice(), View::default());
        let scale = scale(window_size, machine_size);
        let padding = padding(window_size, machine_size, scale);

        Self {
            window_size,
            padding,
            max_travels,
            tool_color: TOOL_COLOR,
            tool_size: max_travels[0].abs() / 40.0,
            tool_len: max_travels[2].abs() / 2.0,
            view: View::default(),
            scale,
        }
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        self.window_size = [new_size.width as f32, new_size.height as f32];
        let machine_size = machine_size(self.max_travels.as_slice(), self.view);
        self.scale = scale(self.window_size, machine_size);
        self.padding = padding(self.window_size, machine_size, self.scale);
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn set_view(&mut self, view: View) {
        self.view = view;
        self.resize(PhysicalSize {
            width: self.window_size[0] as u32,
            height: self.window_size[1] as u32,
        });
    }

    pub fn toggle_tool(&mut self) {
        self.tool_size = if self.tool_size > 1e-10 {
            0.0
        } else {
            self.max_travels[0].abs() / 40.0
        }
    }
}

// returns rect dims to fit inside the window, but in machine units
fn machine_size(max_travels: &[f32], view: View) -> [f32; 2] {
    match view {
        // use x and y of the machine
        View::Top => [max_travels[0].abs(), max_travels[1].abs()],
        // use projection of the bounding box to get final x and y
        View::Isometric => project_bounding_box(max_travels),
    }
}

// returns the real estate required to project the whole machine cuboid, in machine units
fn project_bounding_box(max_travels: &[f32]) -> [f32; 2] {
    [
        (max_travels[0].abs() + max_travels[1].abs()) / 2.0_f32.sqrt(),
        (max_travels[0].abs() + max_travels[1].abs() + max_travels[2].abs()) / 3.0_f32.sqrt(),
    ]
}

// multiply this to machine units to get the number of pixels
// takes final machine_size, after projection if applicable
fn scale(window_size: [f32; 2], machine_size: [f32; 2]) -> f32 {
    // y / x
    let window_ratio = window_size[1] / window_size[0];
    let machine_ratio = machine_size[1] / machine_size[0];

    let scale = match machine_ratio.total_cmp(&window_ratio) {
        // y of machine is smaller, scale to fit x of machine and shrink in y
        Ordering::Less => window_size[0] / machine_size[0],
        // choose any
        Ordering::Equal => window_size[0] / machine_size[0],
        // y of machine is larger, scale to fit y of machine and shrink in x
        Ordering::Greater => window_size[1] / machine_size[1],
    };

    // reduce scale to compensate for machine boundary thickness on both sides
    scale - MACHINE_BOUNDARY_WIDTH
}

// returns the padding to center the machine view inside the window
// takes final machine_size, after projection if applicable
fn padding(window_size: [f32; 2], machine_size: [f32; 2], scale: f32) -> [f32; 2] {
    [
        (window_size[0] - machine_size[0] * scale) / 2.0,
        (window_size[1] - machine_size[1] * scale) / 2.0,
    ]
}

// start is not included in the iterated output
pub fn points(start: Point, end: Point) -> Box<dyn Iterator<Item = Point>> {
    // relative distance of end point from start
    let dir = end - start;
    // distance between start and end points
    let dist = (dir.x().powi(2) + dir.y().powi(2) + dir.z().powi(2)).sqrt();

    if dist <= SPEED {
        return Box::new([end].into_iter());
    }

    // amount to move each axis by to get next point
    let delta = dir.mul_float(SPEED).div_float(dist);

    let mut current = start;

    Box::new(std::iter::from_fn(move || {
        if current == end {
            return None;
        }

        let next = current + delta;
        let remaining = end - next;

        // use dot product to see if the next point is between start and end
        if remaining.x() * dir.x() + remaining.y() * dir.y() + remaining.z() * dir.z() <= 0.0 {
            current = end;
        } else {
            current = next;
        }

        Some(current)
    }))
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Tool {
    position: [f32; 3],
}

impl Tool {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            }],
        }
    }

    pub fn at_point(point: Point) -> Self {
        Self {
            position: [point.x() as f32, point.y() as f32, point.z() as f32],
        }
    }

    pub fn at_line_end(instance: LineInstance) -> Self {
        Self {
            position: instance.end,
        }
    }
}
