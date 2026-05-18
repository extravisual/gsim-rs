// reference for stroke width:
// https://github.com/KaNaDaAT/vega-webgpu/blob/main/src/shaders/line.wgsl

struct Uniforms {
    window_size: vec2<f32>,
    _pad0: vec2<f32>,
    max_travels: vec4<f32>,
    projection: mat4x4<f32>,
    tool_color: vec4<f32>,
    tool_size: f32,
    tool_len: f32,
    _pad: f32,
    view: u32,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) stroke_width: f32,
};

struct VertexOutput {
    // builtin position means that the value is to be used for clip_position
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

// mark as a valid vertex shader
@vertex
fn vs_main(@builtin(vertex_index) index: u32, in: VertexInput) -> VertexOutput {
    // exactly 0 stroke width is intentional and meant when the vertex is not to be shown
    if in.stroke_width == 0.0 {
        var clipped: VertexOutput;
        clipped.clip_position = vec4<f32>(1.1, 1.1, 1.1, 1.0);
        return clipped;
    }

    let window_size = uniforms.window_size;
    let stroke_width = in.stroke_width;

    // scaled to fit the screen, in pixels
    let start = uniforms.projection * vec4<f32>(in.start, 1.0);
    let end = uniforms.projection * vec4<f32>(in.end, 1.0);

    // unit vector from start to end
    let dir = normalize(end - start);
    // normal vector, to get perpendicular direction, with magnitude of stroke width
    let normal = vec2<f32>(-dir.y, dir.x) * stroke_width / 2.0;

    // 4 vertices to form a rectangular line
    var v1 = vec2<f32>(start.xy - normal);
    var v2 = vec2<f32>(start.xy + normal);
    var v3 = vec2<f32>(end.xy - normal);
    var v4 = vec2<f32>(end.xy + normal);

    let vertices = array(
        v1, v2, v3, v2, v4, v3
    );

    var out: VertexOutput;

    // convert to ndc
    // direction already match ndc
    out.clip_position = vec4<f32>((vertices[index] / window_size * 2.0), 0.0, 1.0);
    out.color = in.color;

    return out;
};

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
