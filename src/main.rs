use chrono::Local;
use include_dir::{include_dir, Dir};
use std::{cell::RefCell, rc::Rc};
use tao::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Fullscreen, WindowBuilder},
};
use wry::{
    http::{header::CONTENT_TYPE, Request, Response},
    WebViewBuilder,
};

#[cfg(target_os = "macos")]
use tao::platform::macos::WindowBuilderExtMacOS;

#[derive(Debug, Clone)]
enum Call {
    Reload(String),
    Unplug,
}

#[derive(Debug, Clone)]
struct Config {
    version: String,
    skip_intro: bool,
}

impl Config {
    pub fn to_url(&self) -> String {
        format!(
            "matrix://localhost?version={}&skipIntro={}&renderer=webgpu",
            self.version, self.skip_intro
        )
    }
}

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

static MATRIX_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/matrix");

fn main() -> wry::Result<()> {
    let now = Local::now();
    let time = now.format("%e-%m-%Y %H:%M:%S");
    println!("Call trans opt: received. {} REC:Log>", time);
    println!("Trace program: running");

    let event_loop = EventLoopBuilder::<Call>::with_user_event().build();
    let event_loop_proxy = event_loop.create_proxy();

    #[cfg(target_os = "macos")]
    let window = WindowBuilder::new()
        .with_title("Matrix")
        .with_focused(true)
        .with_title_hidden(true)
        .with_titlebar_buttons_hidden(true)
        .with_titlebar_transparent(true)
        .with_fullsize_content_view(true)
        .with_fullscreen(Some(Fullscreen::Borderless(None)))
        .build(&event_loop)
        .unwrap();

    #[cfg(not(target_os = "macos"))]
    let window = WindowBuilder::new()
        .with_title("Matrix")
        .with_focused(true)
        .with_fullscreen(Some(Fullscreen::Borderless(None)))
        .build(&event_loop)
        .unwrap();

    let config = Rc::new(RefCell::new(Config {
        version: "classic".to_string(),
        skip_intro: false,
    }));
    let url = config.borrow().to_url();

    let keymaker = include_str!("js/keymaker.js");
    let builder = WebViewBuilder::new()
        .with_custom_protocol("matrix".into(), trainman)
        .with_initialization_script(keymaker)
        .with_accept_first_mouse(true)
        .with_url(url)
        .with_ipc_handler(oracle(event_loop_proxy, config));

    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().unwrap();
        builder.build_gtk(vbox)?
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(event) => match event {
                Call::Unplug => *control_flow = ControlFlow::Exit,
                Call::Reload(url) => {
                    let _ = _webview.load_url(&url);
                }
            },
            _ => {}
        }
    });
}

fn oracle(
    event_loop_proxy: tao::event_loop::EventLoopProxy<Call>,
    config: Rc<RefCell<Config>>,
) -> impl Fn(Request<String>) {
    move |msg| {
        if let Some(event) = handle_msg(msg.body().as_str(), &config) {
            let _ = event_loop_proxy.send_event(event);
        }
    }
}

fn trainman(
    _webview_id: &str,
    request: Request<Vec<u8>>,
) -> Response<std::borrow::Cow<'static, [u8]>> {
    let path = request.uri().path();
    let path = if path == "/" {
        "index.html"
    } else {
        &path[1..]
    };

    let file = match MATRIX_DIR.get_file(path) {
        Some(file) => file,
        None => {
            return Response::builder()
                .header(CONTENT_TYPE, "text/plain")
                .status(404)
                .body(std::borrow::Cow::Owned(
                    "File not found".as_bytes().to_vec(),
                ))
                .unwrap();
        }
    };

    let content = file.contents().to_vec();
    let mime_type = get_mime_type(path);

    Response::builder()
        .header(CONTENT_TYPE, mime_type)
        .body(std::borrow::Cow::Owned(content))
        .unwrap_or_else(|e| {
            Response::builder()
                .header(CONTENT_TYPE, "text/plain")
                .status(500)
                .body(std::borrow::Cow::Owned(e.to_string().as_bytes().to_vec()))
                .unwrap()
        })
}

fn handle_msg(msg: &str, config: &Rc<RefCell<Config>>) -> Option<Call> {
    let mut config = config.borrow_mut();
    match msg {
        "Escape" | "KeyQ" => Some(Call::Unplug),
        "Backquote" => {
            config.skip_intro = !config.skip_intro;
            Some(Call::Reload(config.to_url()))
        }
        c if c.starts_with("Digit") => {
            if let Some(digit_char) = c
                .strip_prefix("Digit")
                .and_then(|s| s.parse::<usize>().ok())
            {
                if let Some(&version) = VERSIONS.get(digit_char) {
                    config.version = version.to_string();
                    return Some(Call::Reload(config.to_url()));
                }
            }
            None
        }
        _ => None,
    }
}

fn get_mime_type(path: &str) -> String {
    mime_guess::from_path(path)
        .first()
        .map(|mime| mime.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}
