struct Uniforms {
    window_size: vec2<f32>,
    padding: vec2<f32>,
    max_travels: vec4<f32>,
    scale: f32,
    view: u32,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) pos: vec3<f32>,
};

struct VertexOutput {
    // builtin position means that the value is to be used for clip_position
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

const color: vec3<f32> = vec3<f32>(0.25, 0.25, 0.25);
const tool_len: f32 = 250.0;
const tool_size: f32 = 25.0;
const SQRT_2: f32 = 1.41421356;
const SQRT_3: f32 = 1.73205081;

fn iso_project(in: vec3<f32>) -> vec2<f32> {
    // the arithemtic operations here assume winit coordinate system
    // positive y and z will make the view go down in the window
    return vec2<f32>(
        (in.x + in.y) / SQRT_2,
        (in.x - in.y - in.z) / SQRT_3,
    );
}

fn clipped() -> VertexOutput {
    var clipped: VertexOutput;

    clipped.clip_position = vec4<f32>(1.1, 1.1, 1.1, 1.1);
    clipped.color = vec3<f32>(0.0, 0.0, 0.0);

    return clipped;
}

// angle must be multiple of 4 as we need to draw 4 triangles for each unit degree to create a
// cylinder
@vertex
fn vs_main(@builtin(vertex_index) index: u32, in: VertexInput) -> VertexOutput {
    // total points = 360 * 4 * 3 = 4320
    // total triangles = 360 * 4
    if index >= 4320 {
        // not possible
        // clip out
        return clipped();
    }

    // triangle num to draw
    let triangle = index / 3;
    // vertex of triangle to draw
    let vertex = index % 3;
    // what face of the cylinder does this triangle draw at a particular degree
    let face = triangle / 360;

    // angle in radians
    let angle = radians(f32(triangle % 360));
    let angle_next = radians(f32(triangle % 360) + 1.0);

    var position: vec3<f32> = in.pos;

    switch vertex + face * 3u {
        case 11 {
            // bottom circle center
            position.x += 0.0;
            position.y += 0.0;
            position.z += 0.0;
        }
        case 10 {
            // bottom circle perimeter point at curent angle
            position.x += tool_size * cos(angle);
            position.y += tool_size * sin(angle);
            position.z += 0.0;
        }
        case 9 {
            // bottom circle perimeter point at next unit degree angle
            position.x += tool_size * cos(angle_next);
            position.y += tool_size * sin(angle_next);
            position.z += 0.0;
        }
        case 8 {
            // first wall triangle bottom
            position.x += tool_size * cos(angle);
            position.y += tool_size * sin(angle);
            position.z += 0.0;
        }
        case 7 {
            // first wall triangle top first
            position.x += tool_size * cos(angle);
            position.y += tool_size * sin(angle);
            position.z += tool_len;
        }
        case 6 {
            // first wall triangle top second
            position.x += tool_size * cos(angle_next);
            position.y += tool_size * sin(angle_next);
            position.z += tool_len;
        }
        case 5 {
            // second wall triangle top
            position.x += tool_size * cos(angle_next);
            position.y += tool_size * sin(angle_next);
            position.z += tool_len;
        }
        case 4 {
            // second wall triangle bottom first
            position.x += tool_size * cos(angle_next);
            position.y += tool_size * sin(angle_next);
            position.z += 0.0;
        }
        case 3 {
            // second wall triangle bottom second
            position.x += tool_size * cos(angle);
            position.y += tool_size * sin(angle);
            position.z += 0.0;
        }
        case 2 {
            // top circle center
            position.x += 0.0;
            position.y += 0.0;
            position.z += tool_len;
        }
        case 1 {
            // top circle perimeter point at curent angle
            position.x += tool_size * cos(angle);
            position.y += tool_size * sin(angle);
            position.z += tool_len;
        }
        case 0u {
            // bottom circle perimeter point at next unit degree angle
            position.x += tool_size * cos(angle_next);
            position.y += tool_size * sin(angle_next);
            position.z += tool_len;
        }
        default {
            return clipped();
        }
    }

    let window_size = uniforms.window_size;
    let padding = uniforms.padding;
    let scale = uniforms.scale;
    let max_travels = uniforms.max_travels;
    let absolute_max_travels = abs(uniforms.max_travels);
    let isometric = uniforms.view == 1;

    // convert position to number of pixels
    position *= scale;
    var clip_position: vec2<f32>;

    if isometric {
        // all values here are in winit window coordinate system
        // down is y positive
        clip_position = iso_project(position);

        // origin is to be on left side of the screen, therefore no x offset
        let y_offset = (window_size.y / 2.0) - ((absolute_max_travels.x - absolute_max_travels.y +
        absolute_max_travels.z) / 2.0) * scale / SQRT_3;

        clip_position.x += padding.x;
        clip_position.y += y_offset;
    } else {
        clip_position = position.xy;

        // position on screen with respect to the window coordinate system(0 on top left corner)
        // without padding
        if max_travels.x >= 0.0 {
            if max_travels.y >= 0.0 {
                // machine zero on lower left corner, all positive vals
                clip_position.x += padding.x;
                clip_position.y = (window_size.y - clip_position.y) - padding.y;
            } else {
                // machine zero on top left corner, negative y vals
                clip_position.x += padding.x;
                clip_position.y = abs(clip_position.y) + padding.y;
            }
        } else {
            if max_travels.y >= 0.0 {
                // machine zero on lower right corner, negative x vals
                clip_position.x = (window_size.x - abs(clip_position.x)) + padding.x;
                clip_position.y += padding.y;
            } else {
                // machine zero on top right corner, all negative vals
                clip_position.x = (window_size.x - abs(clip_position.x)) + padding.x;
                clip_position.y = abs(clip_position.y) + padding.y;
            }
        }
    }

    // flip y to match coordinate system of clip space
    clip_position.x = (clip_position.x / window_size.x) * 2.0 - 1.0;
    clip_position.y = 1.0 - (clip_position.y / window_size.y) * 2.0;

    var out: VertexOutput;

    out.clip_position = vec4<f32>(clip_position, 0.1, 1.0);
    out.color = color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
