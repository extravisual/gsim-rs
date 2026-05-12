//! # Gui
//!
//! Creates a new [`Window`] and renders the simulation in it using the [`wgpu`] graphics API.
//!
//! The render loop receives render job [`Command`]s from the [`Tui`] thread,
//! and sends [`Signal`]s in response, to continue or terminate the [`Tui`] thread.

#[allow(unused_imports)]
use crate::{
    Command, Signal, View,
    geometry::StaticVertices,
    geometry::{Uniforms, Vertex, Vertices},
    parser::Point,
    tool::Tool,
    tui::Tui,
};
use std::sync::{Arc, mpsc::Sender};
use wgpu::{BindGroupLayoutEntry, CurrentSurfaceTexture, util::DeviceExt};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    error::EventLoopError,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle},
    window::{Window, WindowId},
};

/// Maximum number of [`Vertex`] allowed to be used in [`wgpu::Buffer`].
const MAX_VERTICES: u64 = 100_000;

/// Represents the current state of the [`Gui`](crate::gui), owned by the **main thread**.
pub struct Gui {
    /// Sender half of the channel for [`Signal`] to [`Tui`].
    signal: Sender<Signal>,
    /// Maximum travel lengths of the [`Machine`](crate::machine::Machine) being rendered.
    max_travels: Point,
    /// Previously received [`Command`] from [`Tui`].
    last_command: Option<Command>,
    /// Active GPU graphics state. [`None`] before window creation.
    graphics: Option<Graphics>,
    /// Stores any errors that occur during [`Graphics::render`] call.
    error: Option<anyhow::Error>,
    /// Configuration for static [`LineVertex`]s which are toggled by the [`Tui`].
    static_vertices: StaticVertices,
    /// [`winit`] event loop that can receive user events in form of [`Command`]s.
    /// Consumed on [`Gui::run`] call.
    event_loop: Option<EventLoop<Command>>,
}

impl Gui {
    /// Constructs a new [`Gui`],
    /// initializing the [`EventLoop`] ready to receive [`Command`]s and send [`Signal`]s.
    ///
    /// The event loop is configured to block and wait until a new (user of OS) event arrives.
    ///
    /// # Errors
    /// Returns [`EventLoopError`] on failure to build the event loop.
    pub fn build(signal: Sender<Signal>, max_travels: Point) -> Result<Self, EventLoopError> {
        let event_loop = EventLoop::<Command>::with_user_event().build()?;
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

        Ok(Self {
            signal,
            max_travels,
            last_command: None,
            graphics: None,
            error: None,
            static_vertices: StaticVertices::default(),
            event_loop: Some(event_loop),
        })
    }

    /// Returns an [`EventLoopProxy`] for sending [`Command`]s to the [`Gui`] from other threads.
    pub fn create_proxy(&self) -> EventLoopProxy<Command> {
        self.event_loop.as_ref().expect("Run method will consume self, therefore eventloop will always be present if the user has a Gui struct.").create_proxy()
    }

    /// Starts the [`Gui`] by running the [`EventLoop`].
    ///
    /// While exiting, checks if the [`Tui`] is still running, using [`Gui::last_command`],
    /// and sends [`Signal::Stop`] to signal a stop, else checks for any error in [`Command::Stop`].
    ///
    /// # Errors
    /// Returns any error in [`Command::Stop`] from [`Tui`] or [`EventLoopError`],
    /// prioritizing [`Tui`] error.
    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = self.event_loop.take().unwrap();
        let res = event_loop.run_app(&mut self);

        // prioritize tui thread error
        // check if the tui thread is still running, if so, tell it to stop
        match self.last_command {
            // the tui thread signalled main thread to stop because of an error in tui thread
            Some(Command::Stop(Some(e))) => self.error = Some(e),
            Some(Command::Stop(None)) => (),
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
    /// On the first call,
    /// creates [`Window`] and builds [`Graphics`] by blocking till completion.
    ///
    /// On failure to create either of the two,
    /// stores the error in [`Gui::error`] and [`exit`](ActiveEventLoop::exit)s the event loop.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.graphics.is_some() {
            return; // repeat resumed call
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_active(false)
                .with_decorations(false)
                .with_visible(true)
                .with_title("GSim"),
        ) {
            Ok(w) => w,
            Err(e) => {
                self.error = Some(e.into());
                return event_loop.exit();
            }
        };

        let graphics = match pollster::block_on(Graphics::build(
            event_loop.owned_display_handle(),
            Arc::new(window),
            self.max_travels,
            self.static_vertices,
        )) {
            Ok(g) => g,
            Err(e) => {
                self.error = Some(e);
                return event_loop.exit();
            }
        };

        self.graphics = Some(graphics);
    }

    /// Handles [`WindowEvent`]s sent by the OS.
    ///
    /// Ignores any event if [`Graphics`] has not yet been initialized.
    ///
    /// On receiving [`WindowEvent::RedrawRequested`], updates simulation state, and:
    /// - Sends [`Signal::Proceed`] to [`Tui`],
    /// if this redraw completely fulfils the last received [`Command::Render`].
    /// - Requests another redraw to fulfil the last received [`Command::Render`].
    ///
    /// If [`Graphics::render`] fails, stores the error and exits the event loop.
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
                        event_loop.exit()
                    }
                }
            }
            _ => (),
        }
    }

    /// Handles [`Command`]s sent from the [`Tui`] thread.
    ///
    /// Each command alters [`Graphics`] state or exits the loop, for [`Command::Stop`].
    /// Latest command is always stored at the end.
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
                self.static_vertices.toggle_machine_boundary();
                graphics.update_fixed(&self.max_travels, self.fixed_config);
                graphics.window.request_redraw();
            }

            Command::ToggleGrid => {
                self.static_vertices.toggle_grid();
                graphics.update_fixed(&self.max_travels, self.fixed_config);
                graphics.window.request_redraw();
            }

            Command::ToggleOrigin => {
                self.static_vertices.toggle_origin();
                graphics.update_fixed(&self.max_travels, self.fixed_config);
                graphics.window.request_redraw();
            }

            Command::ToggleTool => {
                graphics.toggle_tool();
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

/// GPU state for toothpath simulation.
pub struct Graphics {
    /// Logical connection to a GPU.
    device: wgpu::Device,
    /// Command queue for the `device`.
    queue: wgpu::Queue,
    /// Rendering surface created from a [`Window`].
    /// Since the surface holds a reference to the [`Window`] it was created from,
    /// the window is kept alive as long as the surface.
    surface: wgpu::Surface<'static>,
    /// Description of a [`Surface`](wgpu::Surface).
    config: wgpu::SurfaceConfiguration,

    /// Pipeline for rendering [`LineVertex`].
    lines_pipeline: wgpu::RenderPipeline,
    /// Vertex buffer configured to hold [`MAX_VERTICES`] number of [`LineVertex`]s.
    /// [`Self::static_count`] number of static vertices hold the start of this buffer,
    /// which makes upto [`Self::static_offset`] in memory.
    lines_buffer: wgpu::Buffer,
    /// Total number of [`LineVertex`] in [`Self::lines_buffer`],
    /// including static and toolpath vertices.
    lines_count: u32,
    /// Memory offset to write next toolpath vertex to.
    lines_offset: u64,
    /// Number of static vertices (grid, origin, machine boundary).
    static_count: u32,
    /// Memory offset to start toolpath vertices from in [`Self::lines_buffer`].
    static_offset: u64,

    /// Pipeline for rendering [`ToolVertex`].
    tool_pipeline: wgpu::RenderPipeline,
    /// Vertex buffer configured to hold a single [`ToolVertex`].
    tool_buffer: wgpu::Buffer,

    /// [`LineVertex`]s left to be drawn to fulfil the latest [`Command::Render`] from [`Tui`].
    current_vertices: Option<LineVertices>,

    /// Constant data shared across all the [`LineVertex`]s and [`ToolVertex`].
    uniforms: Uniforms,
    /// Read-only buffer containing [`Uniforms`].
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    /// Surface is configured on the first [`Graphics::resize`] call.
    configured: bool,
    /// [`Arc`] keeps the [`Window`] valid for as long as [`Self::surface`] needs,
    /// and lets us use `'static` lifetime with the surface.
    window: Arc<Window>,
}

impl Graphics {
    async fn build(
        handle: OwnedDisplayHandle,
        window: Arc<Window>,
        max_travels: Point,
        static_vertices: StaticVertices,
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

        // ######## Uniforms ########
        //
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

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GSim"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // ######## Line Vertex ########
        //
        // mini program that runs on the gpu
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let lines_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Lines"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });

        let lines_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Lines"),
            layout: Some(&lines_pipeline_layout),
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

        let static_vertices = Vertex::fixed(max_travels, fixed_config);

        let lines_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Lines"),
            size: MAX_VERTICES * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        queue.write_buffer(&lines_buffer, 0, bytemuck::cast_slice(&static_vertices));

        // ######## Tool Vertex ########
        //
        // do not present tool yet
        let shader = device.create_shader_module(wgpu::include_wgsl!("tool.wgsl"));

        let tool_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Tool"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let tool_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tool"),
            layout: Some(&tool_pipeline_layout),
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
            lines_pipeline,
            lines_buffer,
            lines_count: static_vertices.len() as u32,
            lines_offset: bytemuck::cast_slice::<Vertex, u8>(&static_vertices).len() as u64,
            static_count: static_vertices.len() as u32,
            static_offset: bytemuck::cast_slice::<Vertex, u8>(&static_vertices).len() as u64,
            tool_pipeline,
            tool_buffer,
            current_vertices: None,
            uniforms,
            uniform_buffer,
            uniform_bind_group,
            configured: false,
            window,
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

    fn toggle_tool(&mut self) {
        self.uniforms.toggle_tool();

        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }
}
