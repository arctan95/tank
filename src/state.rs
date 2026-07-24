use std::{sync::Arc, time::Instant};

use encase::ShaderType;
use winit::{dpi::PhysicalPosition, window::Window};

use crate::{
    config::MatrixConfig,
    gpu::{uniform_bytes, uniform_size},
    pipeline::MatrixPipeline,
};

pub struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    time_buffer: wgpu::Buffer,
    start_time: Instant,
    frames: i32,
    cursor_position: Option<PhysicalPosition<f64>>,
    config: MatrixConfig,
    pipeline: MatrixPipeline,
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
            .find(|format| {
                matches!(
                    format,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
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

        let config = MatrixConfig::for_version("classic");
        let pipeline =
            MatrixPipeline::new(&device, &queue, &time_buffer, config, surface_config.format)
                .expect("failed to create matrix pipeline");

        let mut state = State {
            window,
            device,
            queue,
            surface,
            surface_config,
            time_buffer,
            start_time: Instant::now(),
            frames: 0,
            cursor_position: None,
            config,
            pipeline,
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

    pub fn set_version(&mut self, version: &str) {
        let skip_intro = self.config.skip_intro;
        let effect = self.config.effect;
        let mut config = MatrixConfig::for_version(version);
        config.skip_intro = skip_intro;
        config.effect = effect;
        self.apply_config(config)
            .expect("failed to rebuild renderer for matrix version");
    }

    pub fn toggle_skip_intro(&mut self) {
        let mut config = self.config;
        config.skip_intro = !config.skip_intro;
        self.apply_config(config)
            .expect("failed to rebuild renderer for intro toggle");
    }

    pub fn set_cursor_position(&mut self, position: PhysicalPosition<f64>) {
        self.cursor_position = Some(position);
    }

    pub fn record_cursor_touch(&mut self) {
        let Some(position) = self.cursor_position else {
            return;
        };
        let width = self.surface_config.width as f32;
        let height = self.surface_config.height as f32;
        if width == 0.0 || height == 0.0 {
            return;
        }
        let normalized = [
            (position.x as f32 / width).clamp(0.0, 1.0),
            (position.y as f32 / height).clamp(0.0, 1.0),
        ];
        self.pipeline.record_touch(
            &self.queue,
            normalized,
            self.start_time.elapsed().as_secs_f32(),
        );
    }

    fn reset_time(&mut self) {
        self.start_time = Instant::now();
        self.frames = 0;
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

        self.pipeline.run(&mut encoder, &surface_view);

        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        surface_texture.present();
        self.frames += 1;
    }

    fn rebuild_targets(&mut self) {
        let size = [self.surface_config.width, self.surface_config.height];
        self.pipeline.build(&self.device, &self.queue, size);
    }

    fn apply_config(&mut self, config: MatrixConfig) -> anyhow::Result<()> {
        self.pipeline = MatrixPipeline::new(
            &self.device,
            &self.queue,
            &self.time_buffer,
            config,
            self.surface_config.format,
        )?;
        self.config = config;
        self.reset_time();
        self.rebuild_targets();
        Ok(())
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

#[derive(Clone, Copy, ShaderType)]
struct TimeUniform {
    seconds: f32,
    frames: i32,
}
