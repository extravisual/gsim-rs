/// Reference for stroke width:
/// https://github.com/KaNaDaAT/vega-webgpu/blob/main/src/shaders/line.wgsl

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct Uniforms {
    window_size: vec2<f32>,
    padding: vec2<f32>,
    max_travels: vec4<f32>,
    tool_color: vec4<f32>,
    tool_size: f32,
    tool_len: f32,
    scale: f32,
    view: u32,
};

struct VertexInput {
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) stroke_width: f32,
};
