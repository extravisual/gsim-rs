//! # Gui
//!
//! Creates a new [`Window`] and renders the simulation in it using the [`wgpu`] graphics API.
//!
//! The render loop receives render job [`Command`]s from the [`Tui`] thread,
//! and sends [`Signal`]s in response, to continue or terminate the [`Tui`] thread.

#[allow(unused_imports)]
use crate::{
    Command, Signal, View,
    geometry::{LineInstance, LineInstances, StaticConfig, ToolInstance, Uniforms},
    machine::HOME_POS,
    parser::Point,
    tui::Tui,
};
use std::{
    mem::size_of,
    sync::{Arc, mpsc::Sender},
};
use wgpu::{BindGroupLayoutEntry, CurrentSurfaceTexture, util::DeviceExt};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    error::EventLoopError,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle},
    window::{Window, WindowId},
};

/// Maximum number of [`LineInstance`]s allowed to be used in the [`Graphics::lines_buffer`].
const MAX_INSTANCES: u64 = 100_000;

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
    /// Configuration for static [`LineInstance`]s which are toggled by the [`Tui`].
    static_config: StaticConfig,
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
            static_config: StaticConfig::default(),
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
            self.static_config,
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
                let instances = LineInstances::new(*summary);

                graphics.add(instances);
                graphics.window.request_redraw();
            }

            Command::SetView(view) => {
                graphics.set_view(*view);
                graphics.window.request_redraw();
            }

            Command::ToggleMachineBoundary => {
                self.static_config.toggle_machine_boundary();
                graphics.update_statics(self.max_travels, self.static_config);
                graphics.window.request_redraw();
            }

            Command::ToggleGrid => {
                self.static_config.toggle_grid();
                graphics.update_statics(self.max_travels, self.static_config);
                graphics.window.request_redraw();
            }

            Command::ToggleOrigin => {
                self.static_config.toggle_origin();
                graphics.update_statics(self.max_travels, self.static_config);
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

    /// Pipeline for rendering [`LineInstance`].
    lines_pipeline: wgpu::RenderPipeline,
    /// Vertex buffer configured to hold [`MAX_INSTANCES`] number of [`LineInstance`]s.
    /// [`Self::static_count`] number of static instances hold the start of this buffer,
    /// which makes upto [`Self::static_offset`] in memory.
    lines_buffer: wgpu::Buffer,
    /// Total number of [`LineInstance`]s in [`Self::lines_buffer`],
    /// including both static and toolpath representing instances.
    lines_count: u32,
    /// Memory offset to write next toolpath [`LineInstance`] to.
    lines_offset: u64,
    /// Number of static [`LineInstance`]s (grid, origin, machine boundary) at the start of
    /// [`Self::lines_buffer`]..
    static_count: u32,
    /// Memory offset to start toolpath [`LineInstance`]s from in [`Self::lines_buffer`].
    static_offset: u64,

    /// Pipeline for rendering the [`ToolInstance`].
    tool_pipeline: wgpu::RenderPipeline,
    /// Vertex buffer configured to hold a single [`ToolInstance`].
    tool_buffer: wgpu::Buffer,

    /// [`LineInstance`]s left to be drawn to fulfil the latest [`Command::Render`] from [`Tui`].
    current_instances: Option<LineInstances>,

    /// Constant data shared across all the [`LineInstance`]s and [`ToolInstance`].
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
    /// Constructs a new [`Graphics`] by initializing all GPU resources, including:
    /// - [`Uniforms`] buffer and bind group, to pass constant data to the [`ToolInstance`] and all
    /// [`LineInstance`]s.
    /// - [`LineInstance`] buffer and pipeline. Writes the static instances,
    /// corresponding to the supplied [`StaticConfig`], to the beginning of [`Self::lines_buffer`].
    /// - [`ToolInstance`] buffer and pipeline. Creates a [`ToolInstance`],
    /// with the tool at [`HOME_POS`], and writes it to [`Self::tool_buffer`].
    ///
    /// Returns [`Error`](anyhow::Error) on failure to create any of the GPU resources.
    async fn build(
        handle: OwnedDisplayHandle,
        window: Arc<Window>,
        max_travels: Point,
        static_config: StaticConfig,
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
                buffers: &[LineInstance::buffer_layout()],
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

        let static_instances = LineInstance::statics(max_travels, static_config);

        let lines_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Lines"),
            size: MAX_INSTANCES * size_of::<LineInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        queue.write_buffer(&lines_buffer, 0, bytemuck::cast_slice(&static_instances));

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
                buffers: &[ToolInstance::buffer_layout()],
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
            size: size_of::<ToolInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let tool = ToolInstance::at_point(HOME_POS);
        queue.write_buffer(&tool_buffer, 0, bytemuck::cast_slice(&[tool]));
        queue.submit([]);

        Ok(Self {
            device,
            queue,
            surface,
            config,
            lines_pipeline,
            lines_buffer,
            lines_count: static_instances.len() as u32,
            lines_offset: bytemuck::cast_slice::<LineInstance, u8>(&static_instances).len() as u64,
            static_count: static_instances.len() as u32,
            static_offset: bytemuck::cast_slice::<LineInstance, u8>(&static_instances).len() as u64,
            tool_pipeline,
            tool_buffer,
            current_instances: None,
            uniforms,
            uniform_buffer,
            uniform_bind_group,
            configured: false,
            window,
        })
    }

    /// Reconfigures [`Self::surface`], updates & rewrites [`Self::uniforms`] to use the new provided size.
    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        let width = new_size.width;
        let height = new_size.height;

        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.configured = true;

            self.uniforms.resize(new_size);
            self.queue.write_buffer(
                &self.uniform_buffer,
                0,
                bytemuck::cast_slice(&[self.uniforms]),
            );
        }
    }

    /// Begins rendering a new move by writing the first [`LineInstance`] to [`Self::lines_buffer`],
    /// and storing the remainder in [`Self::current_instances`] for use in subsequent frames.
    ///
    /// Also, updates the position of [`ToolInstance`] in [`Self::tool_buffer`] to the first
    /// [`LineInstance`] end point.
    ///
    /// Subsequent instances are added in [`Self::update`],
    /// depending on the target geometry of [`LineInstances`]:
    /// - [`LineInstances::Linear`]: The new line instance is merged with the last instance in the
    /// buffer, extending it.
    /// - [`LineInstances::Arc`]: The new line instances are added individually to the buffer.
    fn add(&mut self, mut instances: LineInstances) {
        let first = match &mut instances {
            LineInstances::Linear(lines) => lines.next(),
            LineInstances::Arc(lines) => lines.next(),
        }
        .expect("At least one point is guarranteed, which would be the end point.");

        // since shaders are type agnostic and just see raw bytes,
        // therefore we can only add raw byte slices to the buffer of our types
        self.queue.write_buffer(
            &self.lines_buffer,
            self.lines_offset,
            bytemuck::cast_slice(&[first]),
        );

        // bytemuck cannot infer target type, therefore provide u8
        self.lines_offset += bytemuck::cast_slice::<LineInstance, u8>(&[first]).len() as u64;
        self.lines_count += 1;
        self.current_instances = Some(instances);

        self.queue.write_buffer(
            &self.tool_buffer,
            0,
            bytemuck::cast_slice(&[ToolInstance::at_line_end(first)]),
        );
    }

    /// Uploads the next [`LineInstance`] from [`Self::current_instances`] to
    /// [`Self::lines_buffer`], depending on the target geometry of [`LineInstances`]:
    /// - [`LineInstances::Linear`]: Merges the new line instance with the last instance in the
    /// buffer, extending it.
    /// - [`LineInstances::Arc`]: Appends the new line instance individually to the buffer.
    ///
    /// Also, updates the position of [`ToolInstance`] in [`Self::tool_buffer`] to the new line
    /// instance end point.
    ///
    /// Returns `true` on exhaustion of line instances, indicating [`Gui`] to send a
    /// [`Signal::Proceed`] to the [`Tui`] and receive a new [`Command`].
    /// Returns `false` on adding a new line instance to the buffer (there may be more instances
    /// left to upload to the buffer).
    fn update(&mut self) -> bool {
        // if None, signal has already been sent to retrieve a command from previous block
        // exhaustion
        if let Some(instances) = self.current_instances.as_mut() {
            match instances {
                LineInstances::Linear(lines) => match lines.next() {
                    Some(instance) => {
                        self.update_linear(instance);
                        false
                    }
                    None => {
                        self.current_instances = None;
                        true
                    }
                },
                LineInstances::Arc(lines) => match lines.next() {
                    Some(instance) => {
                        self.update_arc(instance);
                        false
                    }
                    None => {
                        self.current_instances = None;
                        true
                    }
                },
            }
        } else {
            false
        }
    }

    /// Overwrites the provided [`LineInstance`] to the last instance inside [`Self::lines_buffer`].
    ///
    /// Also, updates the position of [`ToolInstance`] in [`Self::tool_buffer`] to the end position
    /// of the provided line instance.
    fn update_linear(&mut self, instance: LineInstance) {
        self.queue.write_buffer(
            &self.lines_buffer,
            self.lines_offset - bytemuck::cast_slice::<LineInstance, u8>(&[instance]).len() as u64,
            bytemuck::cast_slice(&[instance]),
        );

        self.queue.write_buffer(
            &self.tool_buffer,
            0,
            bytemuck::cast_slice(&[ToolInstance::at_line_end(instance)]),
        );
    }

    /// Appends the provided [`LineInstance`] to [`Self::lines_buffer`].
    ///
    /// Also, updates the position of [`ToolInstance`] in [`Self::tool_buffer`] to the end position
    /// of the provided line instance.
    fn update_arc(&mut self, instance: LineInstance) {
        self.queue.write_buffer(
            &self.lines_buffer,
            self.lines_offset,
            bytemuck::cast_slice(&[instance]),
        );

        self.lines_offset += bytemuck::cast_slice::<LineInstance, u8>(&[instance]).len() as u64;
        self.lines_count += 1;

        self.queue.write_buffer(
            &self.tool_buffer,
            0,
            bytemuck::cast_slice(&[ToolInstance::at_line_end(instance)]),
        );
    }

    /// Regenerates the static [`LineInstance`]s with [`LineInstance::statics`],
    /// and overwrites them to the beginning of [`Self::lines_buffer`].
    fn update_statics(&mut self, max_travels: Point, static_config: StaticConfig) {
        let instances = LineInstance::statics(max_travels, static_config);

        // update the fixed vertices
        self.queue
            .write_buffer(&self.lines_buffer, 0, bytemuck::cast_slice(&instances));
    }

    /// Clears all the toolpath [`LineInstance`]s from [`Self::lines_buffer`].
    fn clear(&mut self) {
        self.lines_count = self.static_count;
        self.lines_offset = self.static_offset;
        self.current_instances = None;
    }

    /// Renders a new frame to the [`Self::surface`], drawing the toolpath and tool
    /// by rendering both [`Self::lines_buffer`] and [`Self::tool_buffer`].
    ///
    /// # Errors
    /// Returns [`anyhow::Error`] indicating that the surface is lost.
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

        render_pass.set_pipeline(&self.lines_pipeline);
        render_pass.set_vertex_buffer(0, self.lines_buffer.slice(..));
        render_pass.draw(0..6, 0..self.lines_count);

        render_pass.set_pipeline(&self.tool_pipeline);
        render_pass.set_vertex_buffer(0, self.tool_buffer.slice(..));
        render_pass.draw(0..864, 0..1);

        drop(render_pass);

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        Ok(())
    }

    /// Sets the active [`View`] in [`Self::uniforms`] and uploads the updated uniforms to
    /// [`Self::uniform_buffer`].
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

    /// Toggles the tool visibility in [`Self::uniforms`] and uploads the updated uniforms to
    /// [`Self::uniform_buffer`].
    fn toggle_tool(&mut self) {
        self.uniforms.toggle_tool();

        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }
}
