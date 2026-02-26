use std::sync::Arc;

use chrono::Local;
use tank::{State, Vertex, VERTICES};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
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
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                match key {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Character(c) => match c.as_str() {
                        "q" | "Q" => event_loop.exit(),
                        "`" => {
                            // state.toggle_skip_intro();
                            state.get_window().request_redraw();
                        }
                        n if n.parse::<usize>().is_ok() => {
                            if let Some(&version) = VERSIONS.get(n.parse::<usize>().unwrap()) {
                                // state.set_version(version.to_string());
                                state.get_window().request_redraw();
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
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
