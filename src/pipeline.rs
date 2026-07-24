use crate::{
    config::{EffectKind, MatrixConfig},
    passes::{BloomPass, EndPass, MirrorPass, PalettePass, RainPass},
};

pub(crate) struct MatrixPipeline {
    rain: RainPass,
    bloom: BloomPass,
    effect: EffectPass,
    end: EndPass,
}

enum EffectPass {
    None,
    Mirror(MirrorPass),
    Palette(PalettePass),
}

impl MatrixPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        time_buffer: &wgpu::Buffer,
        config: MatrixConfig,
        surface_format: wgpu::TextureFormat,
    ) -> anyhow::Result<Self> {
        let effect = match config.effect {
            EffectKind::None => EffectPass::None,
            EffectKind::Plain | EffectKind::Palette => {
                EffectPass::Palette(PalettePass::new(device, time_buffer, config))
            }
            EffectKind::Mirror => EffectPass::Mirror(MirrorPass::new(device, queue, time_buffer)),
        };

        Ok(Self {
            rain: RainPass::new(device, queue, time_buffer, config)?,
            bloom: BloomPass::new(device, config),
            effect,
            end: EndPass::new(device, surface_format),
        })
    }

    pub(crate) fn build(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) {
        self.rain.build(device, queue, size);
        self.bloom.build(device, size, self.rain.high_pass_view());
        let output_view = match &mut self.effect {
            EffectPass::None => self.rain.output_view(),
            EffectPass::Mirror(mirror) => {
                mirror.build(
                    device,
                    queue,
                    size,
                    self.rain.output_view(),
                    self.bloom.output_view(),
                );
                mirror.output_view()
            }
            EffectPass::Palette(palette) => {
                palette.build(
                    device,
                    size,
                    self.rain.output_view(),
                    self.bloom.output_view(),
                );
                palette.output_view()
            }
        };
        self.end.build(device, output_view);
    }

    pub(crate) fn run(&self, encoder: &mut wgpu::CommandEncoder, surface_view: &wgpu::TextureView) {
        self.rain.run(encoder);
        self.bloom.run(encoder);
        match &self.effect {
            EffectPass::None => {}
            EffectPass::Mirror(mirror) => mirror.run(encoder),
            EffectPass::Palette(palette) => palette.run(encoder),
        }
        self.end.run(encoder, surface_view);
    }

    pub(crate) fn record_touch(&mut self, queue: &wgpu::Queue, position: [f32; 2], seconds: f32) {
        if let EffectPass::Mirror(mirror) = &mut self.effect {
            mirror.record_touch(queue, position, seconds);
        }
    }
}
