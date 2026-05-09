use std::sync::{Arc, mpsc::Sender};

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle},
    window::{Icon, Window, WindowId},
};

use wgpu::{BindGroupLayoutEntry, CurrentSurfaceTexture, util::DeviceExt};

use crate::{
    Command, Signal,
    app::View,
    geometry::{FixedVertexConfig, Uniforms, Vertex, Vertices},
    parser::Point,
    tool::Tool,
};

const MAX_VERTICES: u64 = 100_000;

pub struct Graphics {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    tool_buffer: wgpu::Buffer,
    tool_pipeline: wgpu::RenderPipeline,
    // number of vertices that make up the grid/axis/boundary
    fixed_vertex_count: u32,
    // total number of vertices, including fixed ones
    vertex_count: u32,
    fixed_offset: u64,
    offset: u64,
    // vertices left to be drawn to complete the current move
    current_vertices: Option<Vertices>,
    uniforms: Uniforms,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    configured: bool,
    // needs to be arc to make sure surface gets static lifetime
    window: Arc<Window>,
}

impl Graphics {
    async fn build(
        handle: OwnedDisplayHandle,
        window: Arc<Window>,
        max_travels: &Point,
        fixed_config: FixedVertexConfig,
    ) -> anyhow::Result<Self> {
        let window_size = window.inner_size();

        // create entry point to the api
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::empty(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: Some(Box::new(handle)),
        });

        // a platform specific window to draw into
        let surface = instance.create_surface(window.clone())?;

        // handle to a physical gpu
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await?;

        // logical connection to a gpu and its command queue
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("GSim"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await?;

        // capabilities of a surface when used with a particular adapter(gpu)
        let surface_caps = surface.get_capabilities(&adapter);

        // try to use srgb or fallback
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(surface_caps.formats.get(0).expect("At least one format must be present, as the adapter is created to be compatible with the surface").clone());

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_size.width,
            height: window_size.height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2, // reasonable default in docs
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };

        // static data to be passed to the shader, that is common to vertices
        let uniforms = Uniforms::new(window_size, max_travels);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GSim"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GSim"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GSim"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // mini program that runs on the gpu
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GSim"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("GSim"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::desc()],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // render every triangle, irrespective of forward facing or not
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0, // use all
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let vertices = Vertex::fixed(max_travels, fixed_config);

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GSim"),
            size: MAX_VERTICES * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&vertices));

        // tool stuff
        // do not present tool yet
        let shader = device.create_shader_module(wgpu::include_wgsl!("tool.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Tool"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let tool_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tool"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Tool::desc()],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let tool_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tool"),
            size: std::mem::size_of::<Tool>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let tool = Tool::at_pos([0.0, 0.0, 0.0]);
        queue.write_buffer(&tool_buffer, 0, bytemuck::cast_slice(&[tool]));
        queue.submit([]);

        Ok(Self {
            device,
            queue,
            surface,
            config,
            window,
            pipeline,
            tool_pipeline,
            vertex_buffer,
            tool_buffer,
            fixed_vertex_count: vertices.len() as u32,
            vertex_count: vertices.len() as u32,
            fixed_offset: bytemuck::cast_slice::<Vertex, u8>(&vertices).len() as u64,
            offset: bytemuck::cast_slice::<Vertex, u8>(&vertices).len() as u64,
            current_vertices: None,
            uniforms,
            uniform_buffer,
            uniform_bind_group: bind_group,
            configured: false,
        })
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        let width = new_size.width;
        let height = new_size.height;

        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.configured = true
        }

        self.uniforms.resize(new_size);
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }

    // add new vertices of a move to the buffer
    fn add(&mut self, mut vertices: Vertices) {
        let first = match &mut vertices {
            Vertices::Linear(vs) => vs.next(),
            Vertices::Arc(vs) => vs.next(),
        }
        .expect("At least one point is guarranteed, which would be the end point.");

        // since shaders are type agnostic and just see raw bytes,
        // therefore we can only add raw byte slices to the buffer of our types
        self.queue.write_buffer(
            &self.vertex_buffer,
            self.offset,
            bytemuck::cast_slice(&[first]),
        );

        // bytemuck cannot infer target type, therefore provide u8
        self.offset += bytemuck::cast_slice::<Vertex, u8>(&[first]).len() as u64;
        self.vertex_count += 1;
        self.current_vertices = Some(vertices);

        self.queue.write_buffer(
            &self.tool_buffer,
            0,
            bytemuck::cast_slice(&[Tool::at_vertex_end(first)]),
        );
    }

    fn update(&mut self) -> bool {
        // if None, signal has already been sent to retrieve a command from previous block
        // exhaustion
        if let Some(vertices) = self.current_vertices.as_mut() {
            match vertices {
                Vertices::Linear(vs) => match vs.next() {
                    Some(vertex) => {
                        self.update_linear(vertex);
                        false
                    }
                    None => {
                        self.current_vertices = None;
                        true
                    }
                },
                Vertices::Arc(vs) => match vs.next() {
                    Some(vertex) => {
                        self.update_arc(vertex);
                        false
                    }
                    None => {
                        self.current_vertices = None;
                        true
                    }
                },
            }
        } else {
            false
        }
    }

    // updates last vertex without adding anything to the buffer
    fn update_linear(&mut self, vertex: Vertex) {
        self.queue.write_buffer(
            &self.vertex_buffer,
            self.offset - bytemuck::cast_slice::<Vertex, u8>(&[vertex]).len() as u64,
            bytemuck::cast_slice(&[vertex]),
        );

        self.queue.write_buffer(
            &self.tool_buffer,
            0,
            bytemuck::cast_slice(&[Tool::at_vertex_end(vertex)]),
        );
    }

    // updates last arc move by extending it and adding a new vertex segment
    fn update_arc(&mut self, vertex: Vertex) {
        self.queue.write_buffer(
            &self.vertex_buffer,
            self.offset,
            bytemuck::cast_slice(&[vertex]),
        );

        self.offset += bytemuck::cast_slice::<Vertex, u8>(&[vertex]).len() as u64;
        self.vertex_count += 1;

        self.queue.write_buffer(
            &self.tool_buffer,
            0,
            bytemuck::cast_slice(&[Tool::at_vertex_end(vertex)]),
        );
    }

    // rewrites updated fixed vertices to the vertex buffer
    fn update_fixed(&mut self, max_travels: &Point, fixed_config: FixedVertexConfig) {
        let vertices = Vertex::fixed(max_travels, fixed_config);

        // update the fixed vertices
        self.queue
            .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
    }

    // clear non fixed vertices from the screen
    fn clear(&mut self) {
        self.vertex_count = self.fixed_vertex_count;
        self.offset = self.fixed_offset;
        self.current_vertices = None;
    }

    fn render(&mut self) -> anyhow::Result<()> {
        if !self.configured {
            return Ok(());
        }

        // surface texture to render to
        let surface_texture = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(surface_texture) => surface_texture,
            CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                // texture out of date with respect to the surface, need reconfiguration
                // still got the texture though
                self.surface.configure(&self.device, &self.config);
                surface_texture
            }
            CurrentSurfaceTexture::Timeout
            | CurrentSurfaceTexture::Occluded
            | CurrentSurfaceTexture::Validation => {
                // skip frame
                return Ok(());
            }
            CurrentSurfaceTexture::Outdated => {
                // texture out of date with respect to the surface, need reconfiguration
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            CurrentSurfaceTexture::Lost => {
                anyhow::bail!("Lost surface, could recreate the resources here")
            }
        };

        // texture cannot be used directly, therefore we need to create a view into it
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("GSim"),
            });

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("GSim"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.01,
                        g: 0.01,
                        b: 0.01,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.draw(0..6, 0..self.vertex_count);

        render_pass.set_pipeline(&self.tool_pipeline);
        render_pass.set_vertex_buffer(0, self.tool_buffer.slice(..));
        render_pass.draw(0..4320, 0..1);

        drop(render_pass);

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        Ok(())
    }

    fn set_view(&mut self, view: View) {
        if self.uniforms.view() == view {
            return;
        } else {
            self.uniforms.set_view(view);
        }

        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }
}

pub struct Gui {
    signal: Sender<Signal>,
    max_travels: Point,
    last_command: Option<Command>,
    graphics: Option<Graphics>,
    // for passing render errors out of the loop
    error: Option<anyhow::Error>,
    fixed_config: FixedVertexConfig,
    event_loop: Option<EventLoop<Command>>,
}

impl Gui {
    pub fn new(signal: Sender<Signal>, max_travels: Point) -> Self {
        let event_loop = EventLoop::<Command>::with_user_event().build().unwrap();
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

        Self {
            signal,
            max_travels,
            last_command: None,
            graphics: None,
            error: None,
            fixed_config: FixedVertexConfig::default(),
            event_loop: Some(event_loop),
        }
    }

    pub fn create_proxy(&self) -> EventLoopProxy<Command> {
        self.event_loop.as_ref().expect("Run method will consume self, therefore eventloop will always be present if the user has a Gui struct.").create_proxy()
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = self.event_loop.take().unwrap();
        let res = event_loop.run_app(&mut self);

        // check if the tui thread is still running, if so, tell it to stop
        match self.last_command {
            // the tui thread signalled main thread to stop
            Some(Command::Stop(_)) => (),
            // tui thread still running, stop it
            _ => self.signal.send(Signal::Stop).unwrap(),
        };

        if let Some(e) = self.error {
            Err(e)
        } else {
            res.map_err(|e| e.into())
        }
    }
}

impl ApplicationHandler<Command> for Gui {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // repeat resumed call
        if self.graphics.is_some() {
            return;
        }

        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_active(false)
                    .with_decorations(false)
                    .with_visible(true)
                    .with_window_icon(Some(
                        Icon::from_rgba(vec![0, 0, 0, 0], 1, 1)
                            .expect("Could not create icon for window"),
                    ))
                    .with_title("GSim"),
            )
            .expect("Could not create a new window");

        let graphics = pollster::block_on(Graphics::build(
            event_loop.owned_display_handle(),
            Arc::new(window),
            &self.max_travels,
            self.fixed_config,
        ))
        .expect("Could not initialize GPU resources");

        self.graphics = Some(graphics);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let graphics = match self.graphics.as_mut() {
            Some(g) => g,
            None => return,
        };

        match event {
            WindowEvent::Resized(size) => graphics.resize(size),
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                let proceed = graphics.update();

                if proceed {
                    self.signal.send(Signal::Proceed).unwrap();
                };

                match graphics.render() {
                    Ok(_) if !proceed => {
                        graphics.window.request_redraw();
                    }
                    Ok(_) => (),
                    Err(e) => {
                        self.error = Some(e);
                        event_loop.exit();
                    }
                }
            }
            _ => (),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Command) {
        let graphics = self.graphics.as_mut().expect("App has been started");

        match &event {
            Command::Render(summary) => {
                let vertices = Vertices::new(*summary);

                graphics.add(vertices);
                graphics.window.request_redraw();
            }

            Command::SetView(view) => {
                graphics.set_view(*view);
                graphics.window.request_redraw();
            }

            Command::ToggleMachineBoundary => {
                self.fixed_config.toggle_machine_boundary();
                graphics.update_fixed(&self.max_travels, self.fixed_config);
                graphics.window.request_redraw();
            }

            Command::ToggleGrid => {
                self.fixed_config.toggle_gird();
                graphics.update_fixed(&self.max_travels, self.fixed_config);
                graphics.window.request_redraw();
            }

            Command::ToggleOrigin => {
                self.fixed_config.toggle_origin();
                graphics.update_fixed(&self.max_travels, self.fixed_config);
                graphics.window.request_redraw();
            }

            Command::Clear => {
                graphics.clear();
                graphics.window.request_redraw();
            }

            Command::Stop(_) => {
                event_loop.exit();
            }
        }

        self.last_command = Some(event);
    }
}
