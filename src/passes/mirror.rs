use encase::ShaderType;
use glam::Vec4;
use wgpu::util::DeviceExt;

use crate::{
    gpu::{buffer_entry, texture_entry, uniform_bytes, RenderTarget},
    texture::Texture,
};

const NUM_TOUCHES: usize = 5;

pub(crate) struct MirrorPass {
    pipeline: wgpu::ComputePipeline,
    config_buffer: wgpu::Buffer,
    scene_buffer: wgpu::Buffer,
    touches_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    time_buffer: wgpu::Buffer,
    camera_texture: Texture,
    touches: [Vec4; NUM_TOUCHES],
    touch_index: usize,
    bind_group: Option<wgpu::BindGroup>,
    output: RenderTarget,
    screen_size: [u32; 2],
}

impl MirrorPass {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        time_buffer: &wgpu::Buffer,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Mirror Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!(
            "../../matrix/shaders/wgsl/mirrorPass.wgsl"
        ));
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Mirror Pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("computeMain"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let config_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mirror Config Uniform Buffer"),
            contents: &uniform_bytes(&MirrorConfigUniform { unused: 0.0 }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let scene_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mirror Scene Uniform Buffer"),
            contents: &uniform_bytes(&MirrorSceneUniform {
                screen_aspect_ratio: 1.0,
                camera_aspect_ratio: 1.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let touches = [Vec4::new(0.0, 0.0, -1000.0, 0.0); NUM_TOUCHES];
        let touches_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mirror Touches Uniform Buffer"),
            contents: &uniform_bytes(&MirrorTouchesUniform { touches }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_texture = Texture::empty(device, queue, "Mirror Empty Camera Texture");

        Self {
            pipeline,
            config_buffer,
            scene_buffer,
            touches_buffer,
            sampler,
            time_buffer: time_buffer.clone(),
            camera_texture,
            touches,
            touch_index: 0,
            bind_group: None,
            output: RenderTarget::new_compute(device, [1, 1], "Mirror Output"),
            screen_size: [1, 1],
        }
    }

    pub(crate) fn build(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        primary: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
    ) {
        self.screen_size = size;
        self.output = RenderTarget::new_compute(device, size, "Mirror Output");
        let screen_aspect_ratio = size[0] as f32 / size[1] as f32;
        queue.write_buffer(
            &self.scene_buffer,
            0,
            &uniform_bytes(&MirrorSceneUniform {
                screen_aspect_ratio,
                camera_aspect_ratio: 1.0,
            }),
        );
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mirror Bind Group"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, &self.config_buffer),
                buffer_entry(1, &self.time_buffer),
                buffer_entry(2, &self.scene_buffer),
                buffer_entry(3, &self.touches_buffer),
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                texture_entry(5, primary),
                texture_entry(6, bloom),
                texture_entry(7, &self.camera_texture.view),
                texture_entry(8, &self.output.view),
            ],
        }));
    }

    pub(crate) fn record_touch(&mut self, queue: &wgpu::Queue, position: [f32; 2], seconds: f32) {
        self.touches[self.touch_index] = Vec4::new(position[0], 1.0 - position[1], seconds, 0.0);
        self.touch_index = (self.touch_index + 1) % NUM_TOUCHES;
        queue.write_buffer(
            &self.touches_buffer,
            0,
            &uniform_bytes(&MirrorTouchesUniform {
                touches: self.touches,
            }),
        );
    }

    pub(crate) fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Mirror Compute Pass"),
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

#[derive(Clone, Copy, ShaderType)]
struct MirrorConfigUniform {
    unused: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct MirrorSceneUniform {
    screen_aspect_ratio: f32,
    camera_aspect_ratio: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct MirrorTouchesUniform {
    touches: [Vec4; NUM_TOUCHES],
}
