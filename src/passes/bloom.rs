use encase::ShaderType;
use glam::Vec2;
use wgpu::util::DeviceExt;

use crate::{
    config::MatrixConfig,
    gpu::{buffer_entry, texture_entry, uniform_bytes, RenderTarget},
};

pub(crate) struct BloomPass {
    enabled: bool,
    bloom_size: f32,
    blur_pipeline: wgpu::ComputePipeline,
    combine_pipeline: wgpu::ComputePipeline,
    h_blur_buffer: wgpu::Buffer,
    v_blur_buffer: wgpu::Buffer,
    combine_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    h_blur_pyramid: Vec<RenderTarget>,
    v_blur_pyramid: Vec<RenderTarget>,
    h_blur_bind_groups: Vec<wgpu::BindGroup>,
    v_blur_bind_groups: Vec<wgpu::BindGroup>,
    combine_bind_group: Option<wgpu::BindGroup>,
    output: RenderTarget,
    scaled_size: [u32; 2],
}

impl BloomPass {
    pub(crate) const PYRAMID_HEIGHT: usize = 4;

    pub(crate) fn new(device: &wgpu::Device, config: MatrixConfig) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Bloom Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blur_shader = device.create_shader_module(wgpu::include_wgsl!(
            "../../matrix/shaders/wgsl/bloomBlur.wgsl"
        ));
        let combine_shader = device.create_shader_module(wgpu::include_wgsl!(
            "../../matrix/shaders/wgsl/bloomCombine.wgsl"
        ));
        let blur_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Bloom Blur Pipeline"),
            layout: None,
            module: &blur_shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let combine_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Bloom Combine Pipeline"),
            layout: None,
            module: &combine_shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        let h_blur_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Horizontal Blur Uniform Buffer"),
            contents: &uniform_bytes(&BloomBlurConfigUniform {
                bloom_radius: 2.0,
                direction: Vec2::new(1.0, 0.0),
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let v_blur_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Vertical Blur Uniform Buffer"),
            contents: &uniform_bytes(&BloomBlurConfigUniform {
                bloom_radius: 2.0,
                direction: Vec2::new(0.0, 1.0),
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let combine_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Combine Uniform Buffer"),
            contents: &uniform_bytes(&BloomCombineConfigUniform {
                pyramid_height: Self::PYRAMID_HEIGHT as f32,
                bloom_strength: config.bloom_strength,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self {
            enabled: config.bloom_size > 0.0 && config.bloom_strength > 0.0,
            bloom_size: config.bloom_size,
            blur_pipeline,
            combine_pipeline,
            h_blur_buffer,
            v_blur_buffer,
            combine_buffer,
            sampler,
            h_blur_pyramid: Vec::new(),
            v_blur_pyramid: Vec::new(),
            h_blur_bind_groups: Vec::new(),
            v_blur_bind_groups: Vec::new(),
            combine_bind_group: None,
            output: RenderTarget::new_compute(device, [1, 1], "Bloom Output"),
            scaled_size: [1, 1],
        }
    }

    pub(crate) fn build(
        &mut self,
        device: &wgpu::Device,
        screen_size: [u32; 2],
        high_pass: &wgpu::TextureView,
    ) {
        if !self.enabled {
            self.output = RenderTarget::new_compute(device, [1, 1], "Bloom Disabled Output");
            self.combine_bind_group = None;
            return;
        }

        self.scaled_size = [
            ((screen_size[0] as f32) * self.bloom_size).floor().max(1.0) as u32,
            ((screen_size[1] as f32) * self.bloom_size).floor().max(1.0) as u32,
        ];
        self.h_blur_pyramid =
            make_bloom_pyramid(device, self.scaled_size, "Bloom Horizontal Pyramid");
        self.v_blur_pyramid =
            make_bloom_pyramid(device, self.scaled_size, "Bloom Vertical Pyramid");
        self.output = RenderTarget::new_compute(device, self.scaled_size, "Bloom Output");

        self.h_blur_bind_groups.clear();
        self.v_blur_bind_groups.clear();
        for i in 0..Self::PYRAMID_HEIGHT {
            let src_view = if i == 0 {
                high_pass
            } else {
                &self.h_blur_pyramid[i - 1].view
            };
            self.h_blur_bind_groups.push(self.create_blur_bind_group(
                device,
                &self.h_blur_buffer,
                src_view,
                &self.h_blur_pyramid[i].view,
            ));
            self.v_blur_bind_groups.push(self.create_blur_bind_group(
                device,
                &self.v_blur_buffer,
                &self.h_blur_pyramid[i].view,
                &self.v_blur_pyramid[i].view,
            ));
        }

        self.combine_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Combine Bind Group"),
            layout: &self.combine_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &self.combine_buffer),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(2, &self.v_blur_pyramid[0].view),
                texture_entry(3, &self.v_blur_pyramid[1].view),
                texture_entry(4, &self.v_blur_pyramid[2].view),
                texture_entry(5, &self.v_blur_pyramid[3].view),
                texture_entry(6, &self.output.view),
            ],
        }));
    }

    pub(crate) fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.enabled {
            return;
        }

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Bloom Compute Pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.blur_pipeline);
        for i in 0..Self::PYRAMID_HEIGHT {
            let level_size = bloom_level_size(self.scaled_size, i);
            let dispatch = [level_size[0].div_ceil(32), level_size[1], 1];
            pass.set_bind_group(0, &self.h_blur_bind_groups[i], &[]);
            pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
            pass.set_bind_group(0, &self.v_blur_bind_groups[i], &[]);
            pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
        }

        pass.set_pipeline(&self.combine_pipeline);
        pass.set_bind_group(0, self.combine_bind_group.as_ref().unwrap(), &[]);
        pass.dispatch_workgroups(self.scaled_size[0].div_ceil(32), self.scaled_size[1], 1);
    }

    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.output.view
    }

    fn create_blur_bind_group(
        &self,
        device: &wgpu::Device,
        config_buffer: &wgpu::Buffer,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Blur Bind Group"),
            layout: &self.blur_pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, config_buffer),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(2, input),
                texture_entry(3, output),
            ],
        })
    }
}

#[derive(Clone, Copy, ShaderType)]
struct BloomBlurConfigUniform {
    bloom_radius: f32,
    direction: Vec2,
}

#[derive(Clone, Copy, ShaderType)]
struct BloomCombineConfigUniform {
    pyramid_height: f32,
    bloom_strength: f32,
}

fn make_bloom_pyramid(device: &wgpu::Device, size: [u32; 2], label: &str) -> Vec<RenderTarget> {
    (0..BloomPass::PYRAMID_HEIGHT)
        .map(|level| RenderTarget::new_compute(device, bloom_level_size(size, level), label))
        .collect()
}

fn bloom_level_size(size: [u32; 2], level: usize) -> [u32; 2] {
    let scale = 2_u32.pow(level as u32);
    [(size[0] / scale).max(1), (size[1] / scale).max(1)]
}
