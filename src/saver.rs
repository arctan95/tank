use crate::config::{EffectKind, MatrixConfig};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SaverSettings {
    version: String,
    mirror_enabled: bool,
    skip_intro: bool,
}

impl SaverSettings {
    pub(crate) fn new(version: &str, mirror_enabled: bool, skip_intro: bool) -> Self {
        Self {
            version: version.to_owned(),
            mirror_enabled,
            skip_intro,
        }
    }

    pub(crate) fn config(&self) -> MatrixConfig {
        let mut config = MatrixConfig::for_version(&self.version);
        config.effect = if self.mirror_enabled {
            EffectKind::Mirror
        } else {
            EffectKind::Palette
        };
        config.skip_intro = self.skip_intro;
        config
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        ffi::{c_char, c_void, CStr},
        ptr::NonNull,
        time::Instant,
    };

    use encase::ShaderType;
    use raw_window_handle::{
        AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
        HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
    };

    use crate::{
        gpu::{uniform_bytes, uniform_size},
        pipeline::MatrixPipeline,
    };

    use super::SaverSettings;

    struct AppKitViewHandle {
        ns_view: NonNull<c_void>,
    }

    impl HasDisplayHandle for AppKitViewHandle {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            let handle = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());
            Ok(unsafe { DisplayHandle::borrow_raw(handle) })
        }
    }

    impl HasWindowHandle for AppKitViewHandle {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let handle = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.ns_view));
            Ok(unsafe { WindowHandle::borrow_raw(handle) })
        }
    }

    pub struct SaverState {
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_config: wgpu::SurfaceConfiguration,
        time_buffer: wgpu::Buffer,
        settings: SaverSettings,
        start_time: Instant,
        frames: i32,
        pipeline: MatrixPipeline,
    }

    impl SaverState {
        pub fn new(
            ns_view: *mut c_void,
            width: u32,
            height: u32,
            version: &str,
            mirror_enabled: bool,
            skip_intro: bool,
        ) -> anyhow::Result<Self> {
            let ns_view = NonNull::new(ns_view).ok_or_else(|| anyhow::anyhow!("null NSView"))?;
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let view_handle = AppKitViewHandle { ns_view };
            let surface = unsafe {
                instance
                    .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&view_handle)?)?
            };
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&surface),
                    ..Default::default()
                }))?;
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

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
                width: width.max(1),
                height: height.max(1),
                present_mode: surface_caps.present_modes[0],
                alpha_mode: surface_caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &surface_config);

            let time_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Saver Time Uniform Buffer"),
                size: uniform_size::<TimeUniform>(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let settings = SaverSettings::new(version, mirror_enabled, skip_intro);
            let config = settings.config();
            let mut pipeline =
                MatrixPipeline::new(&device, &queue, &time_buffer, config, surface_config.format)?;
            pipeline.build(
                &device,
                &queue,
                [surface_config.width, surface_config.height],
            );

            Ok(Self {
                surface,
                device,
                queue,
                surface_config,
                time_buffer,
                settings,
                start_time: Instant::now(),
                frames: 0,
                pipeline,
            })
        }

        pub fn apply_settings(
            &mut self,
            version: &str,
            mirror_enabled: bool,
            skip_intro: bool,
        ) -> anyhow::Result<()> {
            let settings = SaverSettings::new(version, mirror_enabled, skip_intro);
            if self.settings == settings {
                return Ok(());
            }

            let config = settings.config();
            let mut pipeline = MatrixPipeline::new(
                &self.device,
                &self.queue,
                &self.time_buffer,
                config,
                self.surface_config.format,
            )?;
            pipeline.build(
                &self.device,
                &self.queue,
                [self.surface_config.width, self.surface_config.height],
            );

            self.settings = settings;
            self.pipeline = pipeline;
            self.start_time = Instant::now();
            self.frames = 0;
            Ok(())
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            let width = width.max(1);
            let height = height.max(1);
            if self.surface_config.width == width && self.surface_config.height == height {
                return;
            }

            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
            self.pipeline
                .build(&self.device, &self.queue, [width, height]);
        }

        pub fn render(&mut self) {
            let surface_texture = match self.surface.get_current_texture() {
                Ok(texture) => texture,
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    self.surface.configure(&self.device, &self.surface_config);
                    return;
                }
                Err(wgpu::SurfaceError::Timeout) => return,
                Err(wgpu::SurfaceError::OutOfMemory | wgpu::SurfaceError::Other) => return,
            };
            let surface_view = surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            let time = TimeUniform {
                seconds: self.start_time.elapsed().as_secs_f32(),
                frames: self.frames,
            };
            self.queue
                .write_buffer(&self.time_buffer, 0, &uniform_bytes(&time));

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Saver Frame Encoder"),
                });
            self.pipeline.run(&mut encoder, &surface_view);
            self.queue.submit([encoder.finish()]);
            surface_texture.present();
            self.frames += 1;
        }
    }

    unsafe fn version_from_ptr(version: *const c_char) -> String {
        if version.is_null() {
            "classic".to_owned()
        } else {
            CStr::from_ptr(version)
                .to_str()
                .unwrap_or("classic")
                .to_owned()
        }
    }

    #[derive(Clone, Copy, ShaderType)]
    struct TimeUniform {
        seconds: f32,
        frames: i32,
    }

    #[no_mangle]
    pub extern "C" fn matrix_saver_new(
        ns_view: *mut c_void,
        width: u32,
        height: u32,
        version: *const c_char,
        mirror_enabled: u8,
        skip_intro: u8,
    ) -> *mut c_void {
        let version = unsafe { version_from_ptr(version) };

        match SaverState::new(
            ns_view,
            width,
            height,
            &version,
            mirror_enabled != 0,
            skip_intro != 0,
        ) {
            Ok(state) => Box::into_raw(Box::new(state)).cast(),
            Err(error) => {
                eprintln!("failed to create Tank saver renderer: {error:?}");
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn matrix_saver_apply_settings(
        state: *mut c_void,
        version: *const c_char,
        mirror_enabled: u8,
        skip_intro: u8,
    ) {
        let version = version_from_ptr(version);
        if let Some(state) = state.cast::<SaverState>().as_mut() {
            if let Err(error) = state.apply_settings(&version, mirror_enabled != 0, skip_intro != 0)
            {
                eprintln!("failed to apply Matrix saver settings: {error:?}");
            }
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn matrix_saver_resize(state: *mut c_void, width: u32, height: u32) {
        if let Some(state) = state.cast::<SaverState>().as_mut() {
            state.resize(width, height);
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn matrix_saver_render(state: *mut c_void) {
        if let Some(state) = state.cast::<SaverState>().as_mut() {
            state.render();
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn matrix_saver_free(state: *mut c_void) {
        if !state.is_null() {
            drop(Box::from_raw(state.cast::<SaverState>()));
        }
    }
}
