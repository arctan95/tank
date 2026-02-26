use std::{borrow::Cow, sync::Arc, time::Instant};

use anyhow::Context as _;
use encase::{ShaderType, UniformBuffer};
use glam::{IVec2, Mat2, Mat4, Vec2};
use image::GenericImageView;
use wgpu::util::DeviceExt;
use winit::window::Window;

const RENDER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const NUM_VERTICES_PER_QUAD: u32 = 6;

pub struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    time_buffer: wgpu::Buffer,
    start_time: Instant,
    frames: i32,
    rain: RainPass,
    end: EndPass,
}

impl State {
    pub async fn new(window: Arc<Window>) -> State {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        let time_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Time Uniform Buffer"),
            size: uniform_size::<TimeUniform>(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let matrix_config = MatrixConfig::default();
        let rain = RainPass::new(&device, &queue, &time_buffer, matrix_config)
            .expect("failed to create rain pass");
        let end = EndPass::new(&device, surface_config.format);

        let mut state = State {
            window,
            device,
            queue,
            surface,
            surface_config,
            time_buffer,
            start_time: Instant::now(),
            frames: 0,
            rain,
            end,
        };
        state.rebuild_targets();
        state
    }

    pub fn get_window(&self) -> &Window {
        &self.window
    }

    pub fn configure_surface(&self) {
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
            self.rebuild_targets();
        }
    }

    pub fn render(&mut self) {
        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.configure_surface();
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => return,
            Err(wgpu::SurfaceError::OutOfMemory) => panic!("surface is out of memory"),
            Err(wgpu::SurfaceError::Other) => return,
        };
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.write_time();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Matrix Frame Encoder"),
            });

        self.rain.run(&mut encoder);
        self.end.run(&mut encoder, &surface_view);

        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        surface_texture.present();
        self.frames += 1;
    }

    fn rebuild_targets(&mut self) {
        let size = [self.surface_config.width, self.surface_config.height];
        self.rain.build(&self.device, &self.queue, size);
        self.end.build(&self.device, self.rain.output_view());
    }

    fn write_time(&self) {
        let time = TimeUniform {
            seconds: self.start_time.elapsed().as_secs_f32(),
            frames: self.frames,
        };
        self.queue
            .write_buffer(&self.time_buffer, 0, &uniform_bytes(&time));
    }
}

struct RainPass {
    grid_size: [u32; 2],
    num_quads: u32,
    _config_buffer: wgpu::Buffer,
    scene_buffer: wgpu::Buffer,
    intro_pipeline: wgpu::ComputePipeline,
    compute_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    intro_bind_group: wgpu::BindGroup,
    compute_bind_group: wgpu::BindGroup,
    render_bind_group: wgpu::BindGroup,
    output: RenderTarget,
    high_pass_output: RenderTarget,
}

impl RainPass {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        time_buffer: &wgpu::Buffer,
        config: MatrixConfig,
    ) -> anyhow::Result<Self> {
        let grid_size = config.grid_size();
        let num_cells = grid_size[0] * grid_size[1];
        let num_quads = if config.volumetric { num_cells } else { 1 };

        let config_uniform = config.to_rain_uniform();
        let config_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Rain Config Uniform Buffer"),
            contents: &uniform_bytes(&config_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let scene_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rain Scene Uniform Buffer"),
            size: uniform_size::<SceneUniform>(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let intro_cells_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rain Intro Cells Storage Buffer"),
            size: (grid_size[0] as u64) * 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let cells_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rain Cells Storage Buffer"),
            size: (num_cells as u64) * 48,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Rain Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let glyph_msdf = Texture::from_bytes(
            device,
            queue,
            include_bytes!("../matrix/assets/matrixcode_msdf.png"),
            "Matrix Code MSDF",
        )?;
        let empty_texture = Texture::empty(device, queue, "Empty Rain Texture");

        let intro_shader =
            shader_from_wgsl(device, "Rain Intro Shader", rain_shader(RainStage::Intro));
        let compute_shader = shader_from_wgsl(
            device,
            "Rain Compute Shader",
            rain_shader(RainStage::Compute),
        );
        let render_shader =
            shader_from_wgsl(device, "Rain Render Shader", rain_shader(RainStage::Render));

        let intro_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Rain Intro Pipeline"),
            layout: None,
            module: &intro_shader,
            entry_point: Some("computeIntro"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Rain Compute Pipeline"),
            layout: None,
            module: &compute_shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rain Render Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vertMain"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fragMain"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: RENDER_FORMAT,
                        blend: Some(additive_blend()),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: RENDER_FORMAT,
                        blend: Some(additive_blend()),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let intro_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Rain Intro Bind Group"),
            layout: &intro_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &config_buffer),
                buffer_entry(1, time_buffer),
                buffer_entry(2, &intro_cells_buffer),
            ],
        });
        let compute_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Rain Compute Bind Group"),
            layout: &compute_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &config_buffer),
                buffer_entry(1, time_buffer),
                buffer_entry(2, &cells_buffer),
                buffer_entry(3, &intro_cells_buffer),
            ],
        });
        let output = RenderTarget::new(device, [1, 1], "Rain Output");
        let high_pass_output = RenderTarget::new(device, [1, 1], "Rain High Pass Output");
        let render_bind_group = Self::create_render_bind_group(
            device,
            &render_pipeline,
            &config_buffer,
            time_buffer,
            &scene_buffer,
            &linear_sampler,
            &glyph_msdf.view,
            &empty_texture.view,
            &cells_buffer,
        );

        Ok(Self {
            grid_size,
            num_quads,
            _config_buffer: config_buffer,
            scene_buffer,
            intro_pipeline,
            compute_pipeline,
            render_pipeline,
            intro_bind_group,
            compute_bind_group,
            render_bind_group,
            output,
            high_pass_output,
        })
    }

    fn build(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) {
        let aspect_ratio = size[0] as f32 / size[1] as f32;
        let screen_size = if aspect_ratio > 1.0 {
            Vec2::new(1.0, aspect_ratio)
        } else {
            Vec2::new(1.0 / aspect_ratio, 1.0)
        };
        let scene = SceneUniform {
            screen_size,
            camera: perspective_rh_zo(90.0_f32.to_radians(), aspect_ratio, 0.0001, 1000.0),
            transform: Mat4::from_translation(glam::vec3(0.0, 0.0, -1.0)),
        };
        queue.write_buffer(&self.scene_buffer, 0, &uniform_bytes(&scene));

        self.output = RenderTarget::new(device, size, "Rain Output");
        self.high_pass_output = RenderTarget::new(device, size, "Rain High Pass Output");
    }

    fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Rain Intro Compute Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.intro_pipeline);
            pass.set_bind_group(0, &self.intro_bind_group, &[]);
            pass.dispatch_workgroups(self.grid_size[0].div_ceil(32), 1, 1);
        }

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Rain Main Compute Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.compute_pipeline);
            pass.set_bind_group(0, &self.compute_bind_group, &[]);
            pass.dispatch_workgroups(self.grid_size[0].div_ceil(32), self.grid_size[1], 1);
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Rain Render Pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.output.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.high_pass_output.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.render_pipeline);
        pass.set_bind_group(0, &self.render_bind_group, &[]);
        pass.draw(0..NUM_VERTICES_PER_QUAD * self.num_quads, 0..1);
    }

    fn output_view(&self) -> &wgpu::TextureView {
        &self.output.view
    }

    #[allow(clippy::too_many_arguments)]
    fn create_render_bind_group(
        device: &wgpu::Device,
        pipeline: &wgpu::RenderPipeline,
        config_buffer: &wgpu::Buffer,
        time_buffer: &wgpu::Buffer,
        scene_buffer: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
        glyph_msdf_view: &wgpu::TextureView,
        empty_texture_view: &wgpu::TextureView,
        cells_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Rain Render Bind Group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, config_buffer),
                buffer_entry(1, time_buffer),
                buffer_entry(2, scene_buffer),
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                texture_entry(4, glyph_msdf_view),
                texture_entry(5, glyph_msdf_view),
                texture_entry(6, empty_texture_view),
                texture_entry(7, empty_texture_view),
                buffer_entry(8, cells_buffer),
            ],
        })
    }
}

struct EndPass {
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bind_group: Option<wgpu::BindGroup>,
}

impl EndPass {
    fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("End Pass Nearest Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let shader =
            device.create_shader_module(wgpu::include_wgsl!("../matrix/shaders/wgsl/endPass.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("End Pass Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertMain"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragMain"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            sampler,
            pipeline,
            bind_group: None,
        }
    }

    fn build(&mut self, device: &wgpu::Device, input: &wgpu::TextureView) {
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("End Pass Bind Group"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(1, input),
            ],
        }));
    }

    fn run(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("End Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
        pass.draw(0..NUM_VERTICES_PER_QUAD, 0..1);
    }
}

struct RenderTarget {
    view: wgpu::TextureView,
}

impl RenderTarget {
    fn new(device: &wgpu::Device, size: [u32; 2], label: &str) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RENDER_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view }
    }
}

struct Texture {
    view: wgpu::TextureView,
}

impl Texture {
    fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        label: &str,
    ) -> anyhow::Result<Self> {
        let img =
            image::load_from_memory(bytes).with_context(|| format!("failed to load {label}"))?;
        let rgba = img.flipv().to_rgba8();
        let dimensions = img.dimensions();
        Self::from_rgba(device, queue, &rgba, dimensions, label)
    }

    fn empty(device: &wgpu::Device, queue: &wgpu::Queue, label: &str) -> Self {
        Self::from_rgba(device, queue, &[255, 255, 255, 255], (1, 1), label).unwrap()
    }

    fn from_rgba(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        dimensions: (u32, u32),
        label: &str,
    ) -> anyhow::Result<Self> {
        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );
        Ok(Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        })
    }
}

#[derive(Clone, Copy)]
struct MatrixConfig {
    animation_speed: f32,
    glyph_sequence_length: f32,
    glyph_texture_grid_size: IVec2,
    glyph_height_to_width: f32,
    brightness_threshold: f32,
    brightness_override: f32,
    brightness_decay: f32,
    cycle_speed: f32,
    cycle_frame_skip: i32,
    fall_speed: f32,
    raindrop_length: f32,
    num_columns: u32,
    base_brightness: f32,
    base_contrast: f32,
    glint_brightness: f32,
    glint_contrast: f32,
    glyph_vertical_spacing: f32,
    glyph_edge_crop: f32,
    volumetric: bool,
    density: f32,
    slant: f32,
    isolate_cursor: bool,
    isolate_glint: bool,
    loops: bool,
    skip_intro: bool,
    high_pass_threshold: f32,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            animation_speed: 1.0,
            glyph_sequence_length: 57.0,
            glyph_texture_grid_size: IVec2::new(8, 8),
            glyph_height_to_width: 1.0,
            brightness_threshold: 0.0,
            brightness_override: 0.0,
            brightness_decay: 1.0,
            cycle_speed: 0.03,
            cycle_frame_skip: 1,
            fall_speed: 0.3,
            raindrop_length: 0.75,
            num_columns: 80,
            base_brightness: -0.5,
            base_contrast: 1.1,
            glint_brightness: -1.5,
            glint_contrast: 2.5,
            glyph_vertical_spacing: 1.0,
            glyph_edge_crop: 0.0,
            volumetric: false,
            density: 1.0,
            slant: 0.0,
            isolate_cursor: true,
            isolate_glint: false,
            loops: false,
            skip_intro: true,
            high_pass_threshold: 0.1,
        }
    }
}

impl MatrixConfig {
    fn grid_size(self) -> [u32; 2] {
        let density = if self.volumetric { self.density } else { 1.0 };
        [
            (self.num_columns as f32 * density).floor() as u32,
            self.num_columns,
        ]
    }

    fn to_rain_uniform(self) -> RainConfigUniform {
        let grid_size = self.grid_size();
        let glyph_transform = Mat2::IDENTITY;
        let slant_scale = 1.0 / ((2.0 * self.slant).sin().abs() * (2.0_f32.sqrt() - 1.0) + 1.0);
        RainConfigUniform {
            animation_speed: self.animation_speed,
            glyph_sequence_length: self.glyph_sequence_length,
            glyph_texture_grid_size: self.glyph_texture_grid_size,
            glyph_height_to_width: self.glyph_height_to_width,
            glyph_transform,
            grid_size: Vec2::new(grid_size[0] as f32, grid_size[1] as f32),
            show_debug_view: 0,
            brightness_threshold: self.brightness_threshold,
            brightness_override: self.brightness_override,
            brightness_decay: self.brightness_decay,
            cursor_brightness: 1.0,
            cycle_speed: self.cycle_speed,
            cycle_frame_skip: self.cycle_frame_skip,
            fall_speed: self.fall_speed,
            has_thunder: 0,
            raindrop_length: self.raindrop_length,
            ripple_scale: 30.0,
            ripple_speed: 0.2,
            ripple_thickness: 0.2,
            ripple_type: -1,
            msdf_px_range: 4.0,
            forward_speed: 0.25,
            base_brightness: self.base_brightness,
            base_contrast: self.base_contrast,
            glint_brightness: self.glint_brightness,
            glint_contrast: self.glint_contrast,
            has_base_texture: 0,
            has_glint_texture: 0,
            glyph_vertical_spacing: self.glyph_vertical_spacing,
            glyph_edge_crop: self.glyph_edge_crop,
            is_polar: 0,
            density: self.density,
            slant_scale,
            slant_vec: Vec2::new(self.slant.cos(), self.slant.sin()),
            volumetric: self.volumetric as i32,
            isolate_cursor: self.isolate_cursor as i32,
            isolate_glint: self.isolate_glint as i32,
            loops: self.loops as i32,
            skip_intro: self.skip_intro as i32,
            high_pass_threshold: self.high_pass_threshold,
        }
    }
}

#[derive(Clone, Copy, ShaderType)]
struct RainConfigUniform {
    animation_speed: f32,
    glyph_sequence_length: f32,
    glyph_texture_grid_size: IVec2,
    glyph_height_to_width: f32,
    glyph_transform: Mat2,
    grid_size: Vec2,
    show_debug_view: i32,
    brightness_threshold: f32,
    brightness_override: f32,
    brightness_decay: f32,
    cursor_brightness: f32,
    cycle_speed: f32,
    cycle_frame_skip: i32,
    fall_speed: f32,
    has_thunder: i32,
    raindrop_length: f32,
    ripple_scale: f32,
    ripple_speed: f32,
    ripple_thickness: f32,
    ripple_type: i32,
    msdf_px_range: f32,
    forward_speed: f32,
    base_brightness: f32,
    base_contrast: f32,
    glint_brightness: f32,
    glint_contrast: f32,
    has_base_texture: i32,
    has_glint_texture: i32,
    glyph_vertical_spacing: f32,
    glyph_edge_crop: f32,
    is_polar: i32,
    density: f32,
    slant_scale: f32,
    slant_vec: Vec2,
    volumetric: i32,
    isolate_cursor: i32,
    isolate_glint: i32,
    loops: i32,
    skip_intro: i32,
    high_pass_threshold: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct TimeUniform {
    seconds: f32,
    frames: i32,
}

#[derive(Clone, Copy, ShaderType)]
struct SceneUniform {
    screen_size: Vec2,
    camera: Mat4,
    transform: Mat4,
}

#[derive(Clone, Copy)]
enum RainStage {
    Intro,
    Compute,
    Render,
}

fn rain_shader(stage: RainStage) -> String {
    let mut source = include_str!("../matrix/shaders/wgsl/rainPass.wgsl").to_owned();
    let replacements: &[(&str, &str)] = match stage {
        RainStage::Intro => &[
            (
                "@group(0) @binding(2) var<storage, read_write> cells_RW",
                "@group(0) @binding(12) var<storage, read_write> cells_RW",
            ),
            (
                "@group(0) @binding(3) var<storage, read_write> introCells_RO",
                "@group(0) @binding(13) var<storage, read_write> introCells_RO",
            ),
            (
                "@group(0) @binding(2) var<uniform> scene",
                "@group(0) @binding(14) var<uniform> scene",
            ),
            (
                "@group(0) @binding(3) var linearSampler",
                "@group(0) @binding(15) var linearSampler",
            ),
            (
                "@group(0) @binding(4) var glyphMSDFTexture",
                "@group(0) @binding(16) var glyphMSDFTexture",
            ),
            (
                "@group(0) @binding(5) var glintMSDFTexture",
                "@group(0) @binding(17) var glintMSDFTexture",
            ),
            (
                "@group(0) @binding(6) var baseTexture",
                "@group(0) @binding(18) var baseTexture",
            ),
            (
                "@group(0) @binding(7) var glintTexture",
                "@group(0) @binding(19) var glintTexture",
            ),
            (
                "@group(0) @binding(8) var<storage, read> cells_RO",
                "@group(0) @binding(20) var<storage, read> cells_RO",
            ),
        ],
        RainStage::Compute => &[
            (
                "@group(0) @binding(2) var<storage, read_write> introCells_RW",
                "@group(0) @binding(12) var<storage, read_write> introCells_RW",
            ),
            (
                "@group(0) @binding(2) var<uniform> scene",
                "@group(0) @binding(14) var<uniform> scene",
            ),
            (
                "@group(0) @binding(3) var linearSampler",
                "@group(0) @binding(15) var linearSampler",
            ),
            (
                "@group(0) @binding(4) var glyphMSDFTexture",
                "@group(0) @binding(16) var glyphMSDFTexture",
            ),
            (
                "@group(0) @binding(5) var glintMSDFTexture",
                "@group(0) @binding(17) var glintMSDFTexture",
            ),
            (
                "@group(0) @binding(6) var baseTexture",
                "@group(0) @binding(18) var baseTexture",
            ),
            (
                "@group(0) @binding(7) var glintTexture",
                "@group(0) @binding(19) var glintTexture",
            ),
            (
                "@group(0) @binding(8) var<storage, read> cells_RO",
                "@group(0) @binding(20) var<storage, read> cells_RO",
            ),
        ],
        RainStage::Render => &[
            (
                "@group(0) @binding(2) var<storage, read_write> introCells_RW",
                "@group(0) @binding(12) var<storage, read_write> introCells_RW",
            ),
            (
                "@group(0) @binding(2) var<storage, read_write> cells_RW",
                "@group(0) @binding(13) var<storage, read_write> cells_RW",
            ),
            (
                "@group(0) @binding(3) var<storage, read_write> introCells_RO",
                "@group(0) @binding(14) var<storage, read_write> introCells_RO",
            ),
        ],
    };
    for (from, to) in replacements {
        source = source.replace(from, to);
    }
    source
}

fn shader_from_wgsl(
    device: &wgpu::Device,
    label: &'static str,
    source: String,
) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
    })
}

fn uniform_bytes<T>(value: &T) -> Vec<u8>
where
    T: ShaderType + encase::private::WriteInto,
{
    let mut buffer = UniformBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

fn uniform_size<T: ShaderType>() -> wgpu::BufferAddress {
    T::min_size().get()
}

fn buffer_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn texture_entry(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn additive_blend() -> wgpu::BlendState {
    let component = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: component,
        alpha: component,
    }
}

fn perspective_rh_zo(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Mat4 {
    Mat4::perspective_rh(fov_y_radians, aspect_ratio, z_near, z_far)
}
