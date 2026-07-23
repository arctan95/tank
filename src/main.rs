use std::sync::Arc;

use chrono::Local;
use tank::State;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, NamedKey, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

const VERSIONS: [&str; 10] = [
    "3d",
    "neomatrixology",
    "megacity",
    "operator",
    "resurrections",
    "paradise",
    "nightmare",
    "trinity",
    "morpheus",
    "bugs",
];

#[derive(Default)]
struct App {
    state: Option<State>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Create window object
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Matrix")
                        .with_fullscreen(Some(Fullscreen::Borderless(None))),
                )
                .unwrap(),
        );

        let state = pollster::block_on(State::new(window.clone()));
        self.state = Some(state);

        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let state = self.state.as_mut().unwrap();
        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                state.render();
                // Emits a new redraw requested event.
                state.get_window().request_redraw();
            }
            WindowEvent::Resized(size) => {
                // Reconfigures the size of the surface. We do not re-render
                // here as this event is always followed up by redraw request.
                state.resize(size);
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: key,
                        physical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match (physical_key, key) {
                (_, Key::Named(NamedKey::Escape)) => event_loop.exit(),
                (_, Key::Character(c)) if matches!(c.as_str(), "q" | "Q") => event_loop.exit(),
                (PhysicalKey::Code(KeyCode::Backquote), _) => {
                    state.toggle_skip_intro();
                    state.get_window().request_redraw();
                }
                (PhysicalKey::Code(code), _) => {
                    let index = match code {
                        KeyCode::Digit0 => Some(0),
                        KeyCode::Digit1 => Some(1),
                        KeyCode::Digit2 => Some(2),
                        KeyCode::Digit3 => Some(3),
                        KeyCode::Digit4 => Some(4),
                        KeyCode::Digit5 => Some(5),
                        KeyCode::Digit6 => Some(6),
                        KeyCode::Digit7 => Some(7),
                        KeyCode::Digit8 => Some(8),
                        KeyCode::Digit9 => Some(9),
                        _ => None,
                    };
                    if let Some(&version) = index.and_then(|index| VERSIONS.get(index)) {
                        state.set_version(version);
                        state.get_window().request_redraw();
                    }
                }
                _ => {}
            },
            _ => (),
        }
    }
}

fn main() {
    env_logger::init();
    let now = Local::now();
    let time = now.format("%e-%m-%Y %H:%M:%S");
    println!("Call trans opt: received. {} REC:Log>", time);
    println!("Trace program: running");

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}
