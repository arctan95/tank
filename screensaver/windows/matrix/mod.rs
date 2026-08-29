// Windows screen saver host for the shared WGPU renderer.

mod settings;

use std::{
    ffi::c_void,
    num::NonZeroIsize,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context};
use raw_window_handle::{RawWindowHandle, Win32WindowHandle};
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, RECT},
        UI::WindowsAndMessaging::{
            GetClientRect, GetSystemMetrics, MessageBoxW, MB_ICONERROR, MB_OK, SM_CXVIRTUALSCREEN,
            SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        },
    },
};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::windows::WindowAttributesExtWindows,
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};

use super::{SaverSettings, SaverState};

const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const MOUSE_EXIT_DISTANCE: f64 = 4.0;
const INPUT_GRACE_PERIOD: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
enum SaverMode {
    Run,
    Preview(isize),
    Configure(Option<isize>),
}

pub fn run() {
    if let Err(error) = run_inner() {
        show_error(&format!("{error:#}"));
    }
}

fn run_inner() -> anyhow::Result<()> {
    let mode = parse_mode();
    if let SaverMode::Configure(owner) = mode {
        return settings::show(owner);
    }

    let event_loop = EventLoop::new().context("failed to create the Windows event loop")?;
    let mut app = SaverApp::new(mode);
    event_loop
        .run_app(&mut app)
        .context("Windows screen saver event loop failed")
}

fn parse_mode() -> SaverMode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(argument) = arguments.first() else {
        return SaverMode::Configure(None);
    };

    let argument = argument.to_ascii_lowercase();
    let argument = argument.trim_start_matches(['/', '-']);
    let (command, inline_handle) = argument
        .split_once(':')
        .map_or((argument, None), |(command, handle)| {
            (command, Some(handle))
        });
    let handle = inline_handle
        .or_else(|| arguments.get(1).map(String::as_str))
        .and_then(parse_hwnd);

    match command {
        "s" => SaverMode::Run,
        "p" => handle.map_or(SaverMode::Configure(None), SaverMode::Preview),
        "c" => SaverMode::Configure(handle),
        _ => SaverMode::Configure(None),
    }
}

fn parse_hwnd(value: &str) -> Option<isize> {
    value
        .trim()
        .parse::<usize>()
        .ok()
        .map(|value| value as isize)
}

struct SaverApp {
    mode: SaverMode,
    state: Option<SaverState>,
    window: Option<Arc<Window>>,
    next_frame: Instant,
    redraw_pending: bool,
    occluded: bool,
    started_at: Instant,
    initial_cursor_position: Option<PhysicalPosition<f64>>,
}

impl SaverApp {
    fn new(mode: SaverMode) -> Self {
        let now = Instant::now();
        Self {
            mode,
            state: None,
            window: None,
            next_frame: now,
            redraw_pending: false,
            occluded: false,
            started_at: now,
            initial_cursor_position: None,
        }
    }

    fn is_full_screen(&self) -> bool {
        matches!(self.mode, SaverMode::Run)
    }

    fn exit_on_cursor_movement(&mut self, position: PhysicalPosition<f64>) -> bool {
        if !self.is_full_screen() {
            return false;
        }

        let Some(initial) = self.initial_cursor_position else {
            self.initial_cursor_position = Some(position);
            return false;
        };
        if self.started_at.elapsed() < INPUT_GRACE_PERIOD {
            self.initial_cursor_position = Some(position);
            return false;
        }

        let x = position.x - initial.x;
        let y = position.y - initial.y;
        x * x + y * y >= MOUSE_EXIT_DISTANCE * MOUSE_EXIT_DISTANCE
    }
}

impl ApplicationHandler for SaverApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let attributes = match window_attributes(self.mode) {
            Ok(attributes) => attributes.with_visible(false),
            Err(error) => {
                show_error(&format!("{error:#}"));
                event_loop.exit();
                return;
            }
        };
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                show_error(&format!(
                    "failed to create the screen saver window: {error}"
                ));
                event_loop.exit();
                return;
            }
        };

        if self.is_full_screen() {
            window.set_cursor_visible(false);
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                show_error(&format!(
                    "failed to create the screen saver surface: {error}"
                ));
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let mut state = match SaverState::new(
            &instance,
            surface,
            size.width,
            size.height,
            settings::load(),
        ) {
            Ok(state) => state,
            Err(error) => {
                show_error(&format!(
                    "failed to create the screen saver renderer: {error:#}"
                ));
                event_loop.exit();
                return;
            }
        };

        window.set_visible(true);
        state.render(|| window.pre_present_notify());

        self.started_at = Instant::now();
        self.next_frame = self.started_at + FRAME_INTERVAL;
        self.window = Some(window);
        self.state = Some(state);
        self.redraw_pending = false;
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let window = self.window.as_ref();

        match event {
            WindowEvent::RedrawRequested => {
                self.redraw_pending = false;
                if !self.occluded {
                    state.render(|| {
                        if let Some(window) = window {
                            window.pre_present_notify();
                        }
                    });
                }
                self.next_frame = Instant::now() + FRAME_INTERVAL;
            }
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    self.next_frame = Instant::now();
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Focused(false) if self.is_full_screen() => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if self.is_full_screen() && event.state == ElementState::Pressed =>
            {
                event_loop.exit();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            }
            | WindowEvent::MouseWheel { .. }
                if self.is_full_screen() =>
            {
                event_loop.exit();
            }
            WindowEvent::Touch(touch)
                if self.is_full_screen() && touch.phase == TouchPhase::Started =>
            {
                event_loop.exit();
            }
            WindowEvent::CursorMoved { position, .. } if self.exit_on_cursor_movement(position) => {
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() || self.occluded {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }

        let now = Instant::now();
        if now >= self.next_frame && !self.redraw_pending {
            if let Some(window) = &self.window {
                window.request_redraw();
                self.redraw_pending = true;
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

fn window_attributes(mode: SaverMode) -> anyhow::Result<WindowAttributes> {
    match mode {
        SaverMode::Run => full_screen_attributes(),
        SaverMode::Preview(parent) => preview_attributes(parent),
        SaverMode::Configure(_) => Err(anyhow!(
            "configuration mode does not create a render window"
        )),
    }
}

fn full_screen_attributes() -> anyhow::Result<WindowAttributes> {
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    if width <= 0 || height <= 0 {
        return Err(anyhow!("Windows returned an invalid virtual desktop size"));
    }

    Ok(Window::default_attributes()
        .with_title("Matrix")
        .with_decorations(false)
        .with_resizable(false)
        .with_position(PhysicalPosition::new(x, y))
        .with_inner_size(PhysicalSize::new(width as u32, height as u32))
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_skip_taskbar(true))
}

fn preview_attributes(parent: isize) -> anyhow::Result<WindowAttributes> {
    let parent = NonZeroIsize::new(parent).ok_or_else(|| anyhow!("invalid preview HWND"))?;
    let mut bounds = RECT::default();
    unsafe {
        GetClientRect(HWND(parent.get() as *mut c_void), &mut bounds)
            .context("failed to query the preview window size")?;
    }
    let width = (bounds.right - bounds.left).max(1) as u32;
    let height = (bounds.bottom - bounds.top).max(1) as u32;
    let handle = RawWindowHandle::Win32(Win32WindowHandle::new(parent));

    Ok(unsafe {
        Window::default_attributes()
            .with_title("Matrix Preview")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(width, height))
            .with_parent_window(Some(handle))
    })
}

fn show_error(message: &str) {
    let message = message.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(message.as_ptr()),
            w!("Matrix"),
            MB_OK | MB_ICONERROR,
        );
    }
}
