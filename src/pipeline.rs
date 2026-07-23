use crate::{
    config::MatrixConfig,
    passes::{BloomPass, EndPass, PalettePass, RainPass},
};

pub(crate) struct MatrixPipeline {
    rain: RainPass,
    bloom: BloomPass,
    palette: PalettePass,
    end: EndPass,
}

impl MatrixPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        time_buffer: &wgpu::Buffer,
        config: MatrixConfig,
        surface_format: wgpu::TextureFormat,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            rain: RainPass::new(device, queue, time_buffer, config)?,
            bloom: BloomPass::new(device, config),
            palette: PalettePass::new(device, time_buffer, config),
            end: EndPass::new(device, surface_format),
        })
    }

    pub(crate) fn build(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) {
        self.rain.build(device, queue, size);
        self.bloom.build(device, size, self.rain.high_pass_view());
        self.palette.build(
            device,
            size,
            self.rain.output_view(),
            self.bloom.output_view(),
        );
        self.end.build(device, self.palette.output_view());
    }

    pub(crate) fn run(&self, encoder: &mut wgpu::CommandEncoder, surface_view: &wgpu::TextureView) {
        self.rain.run(encoder);
        self.bloom.run(encoder);
        self.palette.run(encoder);
        self.end.run(encoder, surface_view);
    }
}
