use glam::Vec3;
use wgpu::util::DeviceExt;

use crate::{
    config::{MatrixConfig, PaletteEntry},
    gpu::{buffer_entry, texture_entry, uniform_bytes, RenderTarget},
};

pub(crate) struct PalettePass {
    pipeline: wgpu::ComputePipeline,
    config_buffer: wgpu::Buffer,
    palette_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    time_buffer: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    output: RenderTarget,
    screen_size: [u32; 2],
}

impl PalettePass {
    pub(crate) fn new(
        device: &wgpu::Device,
        time_buffer: &wgpu::Buffer,
        config: MatrixConfig,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Palette Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!(
            "../../matrix/shaders/wgsl/palettePass.wgsl"
        ));
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Palette Pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let config_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Palette Config Uniform Buffer"),
            contents: &uniform_bytes(&config.to_palette_uniform()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let palette_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Palette Uniform Buffer"),
            contents: bytemuck::cast_slice(&make_palette(config.palette.entries())),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self {
            pipeline,
            config_buffer,
            palette_buffer,
            sampler,
            time_buffer: time_buffer.clone(),
            bind_group: None,
            output: RenderTarget::new_compute(device, [1, 1], "Palette Output"),
            screen_size: [1, 1],
        }
    }

    pub(crate) fn build(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        primary: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
    ) {
        self.screen_size = size;
        self.output = RenderTarget::new_compute(device, size, "Palette Output");
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Palette Bind Group"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &self.config_buffer),
                buffer_entry(1, &self.palette_buffer),
                buffer_entry(2, &self.time_buffer),
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(4, primary),
                texture_entry(5, bloom),
                texture_entry(6, &self.output.view),
            ],
        }));
    }

    pub(crate) fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Palette Compute Pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
        pass.dispatch_workgroups(self.screen_size[0].div_ceil(32), self.screen_size[1], 1);
    }

    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.output.view
    }
}

fn make_palette(entries: &[PaletteEntry]) -> [[f32; 4]; 512] {
    let mut palette = [[0.0; 4]; 512];
    let mut points: Vec<(Vec3, usize)> = entries
        .iter()
        .map(|entry| {
            (
                entry.color,
                (entry.at.clamp(0.0, 1.0) * 511.0).floor() as usize,
            )
        })
        .collect();
    points.sort_by_key(|(_, index)| *index);

    let first = points[0].0;
    let last = points[points.len() - 1].0;
    points.insert(0, (first, 0));
    points.push((last, 511));

    for window in points.windows(2) {
        let (from_color, from_index) = window[0];
        let (to_color, to_index) = window[1];
        let diff = to_index.saturating_sub(from_index);
        if diff == 0 {
            palette[from_index] = [from_color.x, from_color.y, from_color.z, 0.0];
            continue;
        }
        for i in 0..=diff {
            let ratio = i as f32 / diff as f32;
            let color = from_color * (1.0 - ratio) + to_color * ratio;
            palette[from_index + i] = [color.x, color.y, color.z, 0.0];
        }
    }

    palette
}
