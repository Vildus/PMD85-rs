//! Desktop application: a winit window with a wgpu renderer that blits the
//! emulated PMD 85 framebuffer to the screen, driving the machine at 50 Hz.

mod audio;
mod keys;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pmd85_core::machine::Machine;
use pmd85_core::model::Model;
use pmd85_core::vram::{self, ColorProfile, HEIGHT, WIDTH};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use wgpu::util::DeviceExt;

/// Emulated frame rate.
const FRAME_PERIOD: Duration = Duration::from_millis(20);

struct Args {
    model: Model,
    monitor: String,
    rom_module: Option<String>,
    mute: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        model: Model::Pmd853,
        monitor: String::new(),
        rom_module: None,
        mute: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--model" => {
                let v = it.next().expect("--model needs a value");
                args.model = Model::from_str_loose(&v)
                    .unwrap_or_else(|| panic!("unknown model {v:?}"));
            }
            "--monitor" => args.monitor = it.next().expect("--monitor needs a value"),
            "--rom-module" => args.rom_module = Some(it.next().expect("--rom-module needs a value")),
            "--mute" => args.mute = true,
            other => panic!("unknown argument {other:?}"),
        }
    }
    args
}

/// GPU state for one window.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    vram_texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    vertex_buf: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    /// Padded staging rows (wgpu needs 256-byte row alignment).
    staging: Vec<u8>,
}

const STRIDE: u32 = 1280; // next multiple of 256 above 288*4

impl Gpu {
    fn new(
        window: Arc<Window>,
        size: PhysicalSize<u32>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f == &wgpu::TextureFormat::Bgra8Unorm)
            .or_else(|| caps.formats.first().copied())
            .expect("surface reports no formats");
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // Emulated screen as a texture, refreshed from VRAM every frame.
        let vram_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vram"),
            size: wgpu::Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Fullscreen quad (triangle strip). NDC +1 in y is the top of the
        // window; texture uv (0,0) is the top-left texel of the decoded
        // frame, so both must meet for an upright image.
        let vertices: [[f32; 4]; 4] = [
            [-1.0, 1.0, 0.0, 0.0], // top-left
            [1.0, 1.0, 1.0, 0.0],  // top-right
            [-1.0, -1.0, 0.0, 1.0], // bottom-left
            [1.0, -1.0, 1.0, 1.0],  // bottom-right
        ];
        let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniforms"),
            contents: &[0u8; 32], // vec2 scale, vec2 offset, padding
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let view = vram_texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit-group"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buf.as_entire_binding(),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            vram_texture,
            bind_group,
            uniform_buf,
            vertex_buf,
            pipeline,
            staging: vec![0u8; STRIDE as usize * HEIGHT],
        })
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.config.width = size.width.max(1);
        self.config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Upload a decoded framebuffer (WIDTH*HEIGHT*4 bytes) and draw it,
    /// aspect-correct with integer scaling when possible.
    fn draw(&mut self, frame: &[u8]) {
        // Copy into padded rows (wgpu copy alignment requirement).
        for y in 0..HEIGHT {
            let src = y * WIDTH * 4;
            let dst = y * STRIDE as usize;
            self.staging[dst..dst + WIDTH * 4].copy_from_slice(&frame[src..src + WIDTH * 4]);
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.vram_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.staging,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(STRIDE),
                rows_per_image: Some(HEIGHT as u32),
            },
            wgpu::Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
        );

        // Fit the 288x256 screen into the window: integer scale when it
        // fits at least 1:1, otherwise fractional.
        let (w, h) = (self.config.width, self.config.height);
        let scale = (w / WIDTH as u32).min(h / HEIGHT as u32).max(1) as f32;
        let scale_x = WIDTH as f32 * scale / w as f32;
        let scale_y = HEIGHT as f32 * scale / h as f32;
        let uniforms: [[f32; 4]; 2] = [[scale_x, scale_y, 0.0, 0.0], [0.0; 4]];
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(&uniforms));

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            status => {
                log::warn!("surface status {status:?}, skipping frame");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
            pass.draw(0..4, 0..1);
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
    }
}

const SHADER: &str = r#"
struct Uniforms {
    scale: vec2f,
    offset: vec2f,
};
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> uni: Uniforms;

struct VsOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vs_main(@location(0) pos: vec2f, @location(1) uv: vec2f) -> VsOut {
    var out: VsOut;
    out.pos = vec4f(pos * uni.scale + uni.offset, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    return textureSample(tex, samp, in.uv);
}
"#;

struct App {
    machine: Machine,
    profile: ColorProfile,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    decode_buf: Vec<u8>,
    /// Speaker output (None when muted or no device available).
    audio: Option<audio::SpeakerOut>,
    /// Wall-clock anchor for the next emulated frame (50 Hz).
    next_frame: Instant,
    /// Emulated frames owed to the machine (catch-up after a stall).
    pending: u32,
    /// Total emulated frames (for pace diagnostics).
    frames_done: u32,
    /// App start, for pace diagnostics.
    start_time: Instant,
}

impl App {
    fn new() -> Self {
        let args = parse_args();
        let monitor_path = if args.monitor.is_empty() {
            format!(
                "{}{}",
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../Rom/"),
                args.model.default_monitor()
            )
        } else {
            args.monitor
        };
        let monitor = std::fs::read(&monitor_path)
            .unwrap_or_else(|e| panic!("cannot read monitor ROM {monitor_path:?}: {e}"));
        let rom_module = args
            .rom_module
            .map(|p| std::fs::read(&p).unwrap_or_else(|e| panic!("cannot read ROM module {p:?}: {e}")));
        let audio = if args.mute {
            None
        } else {
            audio::SpeakerOut::new()
        };
        Self {
            machine: Machine::new(args.model, &monitor, rom_module),
            profile: ColorProfile::Rgb,
            window: None,
            gpu: None,
            decode_buf: Vec::new(),
            audio,
            next_frame: Instant::now(),
            pending: 0,
            frames_done: 0,
            start_time: Instant::now(),
        }
    }

    fn step_and_draw(&mut self) {
        // Emulate the frames owed (usually one, more after a stall) and
        // present the decoded screen.
        let frames = self.pending.max(1);
        self.pending = 0;
        for _ in 0..frames {
            self.machine.step_frame();
        }
        // Feed the speaker: expand this frame's edges into samples and
        // hand them to the audio thread (dropped silently when muted or
        // no device - the edge log in the core is bounded either way).
        let edges = self.machine.take_speaker_edges();
        if let Some(audio) = &mut self.audio {
            audio.submit(edges, self.machine.bus.total_cycles());
        }
        // Pace diagnostics (visible with RUST_LOG=debug).
        self.frames_done += frames;
        if log::log_enabled!(log::Level::Debug) && self.frames_done % 250 == 0 {
            let elapsed = self.start_time.elapsed().as_secs_f64();
            log::debug!(
                "{:5} emulated frames in {:6.1}s wall ({:.1} fps, {} behind pending)",
                self.frames_done,
                elapsed,
                self.frames_done as f64 / elapsed,
                frames.saturating_sub(1)
            );
        }
        if let Some(gpu) = &mut self.gpu {
            let buf = vram::decode_into(&self.machine.bus.memory, self.profile, &mut self.decode_buf);
            gpu.draw(buf);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = winit::window::Window::default_attributes()
            .with_title("PMD 85")
            .with_inner_size(PhysicalSize::new(WIDTH as u32 * 2, HEIGHT as u32 * 2));
        let window = Arc::new(event_loop.create_window(attrs).expect("cannot create window"));
        let gpu = Gpu::new(window.clone(), window.inner_size()).expect("cannot initialize wgpu");
        self.window = Some(window);
        self.gpu = Some(gpu);
        self.next_frame = Instant::now();
        self.pending = 0;
        // Wake up for the first emulated frame; ControlFlow::Wait would
        // sleep until an OS event arrives.
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame + FRAME_PERIOD));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key) = keys::map(&event) {
                    self.machine.bus.keyboard.set_key(key, event.state.is_pressed());
                }
            }
            WindowEvent::Focused(false) => {
                self.machine.bus.keyboard.reset();
            }
            WindowEvent::RedrawRequested => self.step_and_draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = &self.window else {
            return;
        };
        // Real-time pacing at 50 Hz. The event loop must be given an
        // explicit wake-up deadline: with ControlFlow::Wait it would sleep
        // until an OS event (mouse move, key press), and the emulator
        // would only run while the user generates events.
        let now = Instant::now();
        if now.duration_since(self.next_frame) >= FRAME_PERIOD {
            // How many frames we owe, bounded so a long stall (window
            // drag, resize, slow frame) does not fast-forward in a burst.
            let owed = ((now.duration_since(self.next_frame).as_millis() / 20) as u32)
                .clamp(1, 6);
            self.pending = (self.pending + owed).min(8);
            self.next_frame += FRAME_PERIOD * owed;
            if now.duration_since(self.next_frame) >= FRAME_PERIOD {
                // Too far behind to catch up gracefully: resynchronize.
                self.next_frame = now;
            }
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame + FRAME_PERIOD));
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let event_loop = EventLoop::new().expect("cannot create event loop");
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("event loop failed");
}
