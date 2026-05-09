use crate::geometry::Vertex;

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

    pub fn at_pos(pos: [f32; 3]) -> Self {
        Self {
            position: [pos[0], pos[1], pos[2]],
        }
    }

    pub fn at_vertex_end(vertex: Vertex) -> Self {
        Self::at_pos(vertex.end)
    }
}
