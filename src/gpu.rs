use std::borrow::Cow;

use encase::{ShaderType, UniformBuffer};
use glam::Mat4;

pub(crate) const RENDER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const NUM_VERTICES_PER_QUAD: u32 = 6;

pub(crate) struct RenderTarget {
    pub(crate) view: wgpu::TextureView,
}

impl RenderTarget {
    pub(crate) fn new(device: &wgpu::Device, size: [u32; 2], label: &str) -> Self {
        Self::with_usage(
            device,
            size,
            label,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
    }

    pub(crate) fn new_compute(device: &wgpu::Device, size: [u32; 2], label: &str) -> Self {
        Self::with_usage(
            device,
            size,
            label,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::STORAGE_BINDING,
        )
    }

    pub(crate) fn with_usage(
        device: &wgpu::Device,
        size: [u32; 2],
        label: &str,
        usage: wgpu::TextureUsages,
    ) -> Self {
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
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RainStage {
    Intro,
    Compute,
    Render,
}

pub(crate) fn rain_shader(stage: RainStage) -> String {
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

pub(crate) fn shader_from_wgsl(
    device: &wgpu::Device,
    label: &'static str,
    source: String,
) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
    })
}

pub(crate) fn uniform_bytes<T>(value: &T) -> Vec<u8>
where
    T: ShaderType + encase::private::WriteInto,
{
    let mut buffer = UniformBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

pub(crate) fn uniform_size<T: ShaderType>() -> wgpu::BufferAddress {
    T::min_size().get()
}

pub(crate) fn buffer_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

pub(crate) fn texture_entry(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

pub(crate) fn additive_blend() -> wgpu::BlendState {
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

pub(crate) fn perspective_rh_zo(
    fov_y_radians: f32,
    aspect_ratio: f32,
    z_near: f32,
    z_far: f32,
) -> Mat4 {
    Mat4::perspective_rh(fov_y_radians, aspect_ratio, z_near, z_far)
}
