#![cfg(windows)]
// Many Win32 calls return a `Result`/`BOOL` we intentionally ignore in a screen
// saver host (best-effort UI/registry operations).
#![allow(unused_must_use)]

// Windows screen saver host for the Matrix rain animation.
//
// Screen savers are plain executables with a `.scr` extension invoked by the
// system with one of the following command line switches:
//   /s            run the screen saver (full screen)
//   /p <HWND>     preview inside the small parent window identified by HWND
//   /c            show the settings dialog
//   /c:<HWND>     show the settings dialog (HWND of the owning window is given)
//
// The renderer itself lives in `tank::saver` and is shared with the macOS
// implementation (see screensaver/macos). Here we only provide the Win32
// window, message loop and settings storage.

use std::ffi::c_void;

use raw_window_handle::{
    HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, Win32WindowHandle,
    WindowHandle, WindowsDisplayHandle,
};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::Sleep;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use tank::saver::SaverState;

const REG_KEY: PCWSTR = w!("Software\\Tank\\Screensaver");
const CLASS_NAME: PCWSTR = w!("TankMatrixSaver");
const CLASS_NAME_DLG: PCWSTR = w!("TankMatrixSaverSettings");

// ---- Control IDs for settings dialog ----
const IDC_VERSION_COMBO: i32 = 1001;
const IDC_MIRROR: i32 = 1002;
const IDC_SKIP_INTRO: i32 = 1003;
const IDC_OK: i32 = 1004;
const IDC_CANCEL: i32 = 1005;

const VERSIONS: &[&str] = &[
    "classic",
    "megacity",
    "neomatrixology",
    "operator",
    "nightmare",
    "paradise",
    "resurrections",
    "trinity",
    "morpheus",
    "bugs",
    "palimpsest",
    "twilight",
    "holoplay",
    "3d",
];

// Dialog DPI scaling
static DPI_SCALE: std::sync::Mutex<f32> = std::sync::Mutex::new(1.0);

fn scale(value: i32) -> i32 {
    let scale = *DPI_SCALE.lock().unwrap();
    ((value as f32) * scale) as i32
}

static RUNNING: AtomicBool = AtomicBool::new(true);
static STATE: Mutex<Option<Box<SaverState>>> = Mutex::new(None);

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct Settings {
    version: String,
    mirror: bool,
    skip_intro: bool,
}

impl Settings {
    fn load() -> Self {
        let mut version = String::from("classic");
        let mut mirror: u32 = 0;
        let mut skip_intro: u32 = 0;

        let mut key = HKEY::default();
        if unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                REG_KEY,
                None,
                KEY_READ,
                &mut key,
            )
        }
        .is_ok()
        {
            let mut buf = [0u16; 64];
            let mut size = (buf.len() as u32) * 2;
            if unsafe {
                RegQueryValueExW(
                    key,
                    w!("Version"),
                    None,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut size),
                )
            }
            .is_ok()
            {
                let len = (size as usize) / 2;
                version = String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_owned();
            }
            let mut vsize = 4u32;
            unsafe {
                RegQueryValueExW(
                    key,
                    w!("Mirror"),
                    None,
                    None,
                    Some(&mut mirror as *mut u32 as *mut u8),
                    Some(&mut vsize),
                )
            }
            .ok();
            let mut isize = 4u32;
            unsafe {
                RegQueryValueExW(
                    key,
                    w!("SkipIntro"),
                    None,
                    None,
                    Some(&mut skip_intro as *mut u32 as *mut u8),
                    Some(&mut isize),
                )
            }
            .ok();
        }

        Settings {
            version: if version.is_empty() {
                "classic".to_owned()
            } else {
                version
            },
            mirror: mirror != 0,
            skip_intro: skip_intro != 0,
        }
    }

    fn save(&self) {
        let mut key = HKEY::default();
        if unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                REG_KEY,
                None,
                windows::core::PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
        }
        .is_ok()
        {
            let wide: Vec<u16> = self.version.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(wide.as_ptr().cast(), wide.len() * 2)
            };
            unsafe { RegSetValueExW(key, w!("Version"), None, REG_SZ, Some(bytes)) }.ok();

            let mirror: u32 = if self.mirror { 1 } else { 0 };
            let mirror_bytes = mirror.to_le_bytes();
            unsafe {
                RegSetValueExW(key, w!("Mirror"), None, REG_DWORD, Some(&mirror_bytes))
            }
            .ok();

            let skip: u32 = if self.skip_intro { 1 } else { 0 };
            let skip_bytes = skip.to_le_bytes();
            unsafe {
                RegSetValueExW(key, w!("SkipIntro"), None, REG_DWORD, Some(&skip_bytes))
            }
            .ok();
        }
    }
}

// Wraps an HWND so it can be handed to wgpu as a window surface target.
struct HwndHandle {
    hwnd: HWND,
}

impl HasDisplayHandle for HwndHandle {
    fn display_handle(
        &self,
    ) -> core::result::Result<raw_window_handle::DisplayHandle<'_>, HandleError> {
        let handle = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(handle) })
    }
}

impl HasWindowHandle for HwndHandle {
    fn window_handle(
        &self,
    ) -> core::result::Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let nz = std::num::NonZeroIsize::new(self.hwnd.0 as isize)
            .ok_or(raw_window_handle::HandleError::NotSupported)?;
        let mut handle = Win32WindowHandle::new(nz);
        handle.hinstance = None;
        // SAFETY: the HWND is valid for the lifetime of the surface and wgpu only
        // reads it during surface creation.
        Ok(unsafe { WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::Win32(handle)) })
    }
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            RUNNING.store(false, Ordering::SeqCst);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_CLOSE => {
            RUNNING.store(false, Ordering::SeqCst);
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            RUNNING.store(false, Ordering::SeqCst);
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            // Exit once the pointer moves more than a few pixels from where the
            // saver started (the usual screen saver dismissal rule).
            static LAST: Mutex<Option<(i32, i32)>> = Mutex::new(None);
            let x = ((lparam.0 as u32) & 0xffff) as i16 as i32;
            let y = (((lparam.0 as u32) >> 16) & 0xffff) as i16 as i32;
            let mut last = LAST.lock().unwrap();
            match *last {
                None => *last = Some((x, y)),
                Some((px, py)) => {
                    if (x - px).abs() > 5 || (y - py).abs() > 5 {
                        RUNNING.store(false, Ordering::SeqCst);
                        let _ = unsafe { DestroyWindow(hwnd) };
                    }
                }
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let w = (lparam.0 as u32) & 0xffff;
            let h = ((lparam.0 as u32) >> 16) & 0xffff;
            if let Some(state) = STATE.lock().unwrap().as_mut() {
                state.resize(w.max(1), h.max(1));
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn register_class() -> anyhow::Result<HINSTANCE> {
    let hmodule = unsafe { GetModuleHandleW(None)? };
    let hinstance = HINSTANCE(hmodule.0 as *mut c_void);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hbrBackground: HBRUSH(0 as *mut c_void), // black, painted by the renderer
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };
    Ok(hinstance)
}

fn create_render_state(hwnd: HWND, settings: &Settings) -> anyhow::Result<()> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) };
    let width = (rect.right - rect.left).max(1) as u32;
    let height = (rect.bottom - rect.top).max(1) as u32;

    let handle = HwndHandle { hwnd };
    let state = tank::saver::build_state(
        &handle,
        width,
        height,
        &settings.version,
        settings.mirror,
        settings.skip_intro,
    )?;
    *STATE.lock().unwrap() = Some(Box::new(state));
    Ok(())
}

fn run_loop() -> anyhow::Result<()> {
    RUNNING.store(true, Ordering::SeqCst);
    let mut msg = MSG::default();
    while RUNNING.load(Ordering::SeqCst) {
        if unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.into() {
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        } else if let Some(state) = STATE.lock().unwrap().as_mut() {
            state.render();
        } else {
            // No renderer yet (still initializing); yield to avoid a busy spin.
            unsafe { Sleep(8) };
        }
    }
    Ok(())
}

fn run_fullscreen(settings: Settings) -> anyhow::Result<()> {
    let hinstance = register_class()?;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            CLASS_NAME,
            w!("Tank Matrix Saver"),
            WS_POPUP | WS_VISIBLE,
            0,
            0,
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
            None,
            None,
            Some(hinstance),
            None,
        )?
    };
    create_render_state(hwnd, &settings)?;
    // Debug-only: force a specific version to reproduce renderer panics headless.
    if let Ok(test_version) = std::env::var("TANK_TEST_VERSION") {
        if let Some(state) = STATE.lock().unwrap().as_mut() {
            let _ = state.apply_settings(&test_version, settings.mirror, settings.skip_intro);
        }
        for _ in 0..120 {
            if let Some(state) = STATE.lock().unwrap().as_mut() {
                state.render();
            }
            unsafe { Sleep(16) };
        }
        return Ok(());
    }
    run_loop()
}

fn run_preview(parent: HWND, settings: Settings) -> anyhow::Result<()> {
    let hinstance = register_class()?;
    // Child window fills the preview area provided by the control panel.
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            CLASS_NAME,
            w!("Tank Matrix Saver Preview"),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(hinstance),
            None,
        )?
    };
    let mut rect = RECT::default();
    unsafe { GetClientRect(parent, &mut rect) };
    unsafe { MoveWindow(hwnd, 0, 0, rect.right, rect.bottom, true) };
    create_render_state(hwnd, &settings)?;
    // Debug-only: force a specific version to reproduce renderer panics headless.
    if let Ok(test_version) = std::env::var("TANK_TEST_VERSION") {
        if let Some(state) = STATE.lock().unwrap().as_mut() {
            let _ = state.apply_settings(&test_version, settings.mirror, settings.skip_intro);
        }
        for _ in 0..120 {
            if let Some(state) = STATE.lock().unwrap().as_mut() {
                state.render();
            }
            unsafe { Sleep(16) };
        }
        return Ok(());
    }
    run_loop()
}

// ---- Settings dialog (Win11 Modern Style) ----------------------------------

// Result captured from the settings dialog before the window is destroyed.
// Written by `dlg_proc` on OK/Cancel, read by `show_settings_dialog` after the
// message loop exits.
static DIALOG_RESULT: std::sync::OnceLock<std::sync::Mutex<Option<Settings>>> =
    std::sync::OnceLock::new();
static DIALOG_CONFIRMED: std::sync::OnceLock<std::sync::Mutex<bool>> =
    std::sync::OnceLock::new();

/// Read control states and save settings. Called from inside `dlg_proc` while
/// the dialog window (and its children) are still alive.
fn capture_and_save_settings(hwnd: HWND) {
    let mut settings = Settings::load();

    // Version combo box.
    if let Ok(combo) = unsafe { GetDlgItem(Some(hwnd), IDC_VERSION_COMBO) } {
        let sel = unsafe { SendMessageW(combo, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))) };
        if sel.0 >= 0 && (sel.0 as usize) < VERSIONS.len() {
            settings.version = VERSIONS[sel.0 as usize].to_owned();
        }
    }
    // Mirror checkbox.
    if let Ok(ctrl) = unsafe { GetDlgItem(Some(hwnd), IDC_MIRROR) } {
        settings.mirror =
            unsafe { SendMessageW(ctrl, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))) }.0 != 0;
    }
    // Skip-intro checkbox.
    if let Ok(ctrl) = unsafe { GetDlgItem(Some(hwnd), IDC_SKIP_INTRO) } {
        settings.skip_intro =
            unsafe { SendMessageW(ctrl, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))) }.0 != 0;
    }

    settings.save();

    // Stash the resulting settings so show_settings_dialog can report success.
    if let Some(result) = DIALOG_RESULT.get() {
        *result.lock().unwrap() = Some(settings);
    }
}

// Helper to create a static text label
fn create_label(
    parent: HWND,
    hinst: HINSTANCE,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> std::result::Result<HWND, windows::core::Error> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateWindowExW(
            Default::default(),
            w!("Static"),
            PCWSTR(wide.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            scale(x),
            scale(y),
            scale(w),
            scale(h),
            Some(parent),
            None,
            Some(hinst),
            None,
        )
    }
}

// Helper to create a checkbox
fn create_checkbox(
    parent: HWND,
    hinst: HINSTANCE,
    id: i32,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    checked: bool,
) -> std::result::Result<HWND, windows::core::Error> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            w!("Button"),
            PCWSTR(wide.as_ptr()),
            WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
            scale(x),
            scale(y),
            scale(w),
            scale(h),
            Some(parent),
            Some(HMENU((id as isize) as *mut c_void)),
            Some(hinst),
            None,
        )?
    };
    if checked {
        unsafe {
            SendMessageW(
                hwnd,
                BM_SETCHECK,
                Some(WPARAM(BST_CHECKED.0 as usize)),
                Some(LPARAM(0)),
            )
        };
    }
    Ok(hwnd)
}

// Helper to create a push button (Win11 style)
fn create_push_button(
    parent: HWND,
    hinst: HINSTANCE,
    id: i32,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    is_default: bool,
) -> std::result::Result<HWND, windows::core::Error> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let style = if is_default {
        BS_DEFPUSHBUTTON as u32
    } else {
        BS_PUSHBUTTON as u32
    };
    unsafe {
        CreateWindowExW(
            Default::default(),
            w!("Button"),
            PCWSTR(wide.as_ptr()),
            WINDOW_STYLE(style | WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
            scale(x),
            scale(y),
            scale(w),
            scale(h),
            Some(parent),
            Some(HMENU((id as isize) as *mut c_void)),
            Some(hinst),
            None,
        )
    }
}

// Helper to create a combobox
fn create_combobox(
    parent: HWND,
    hinst: HINSTANCE,
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    items: &[&str],
    selected: Option<usize>,
) -> std::result::Result<HWND, windows::core::Error> {
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            w!("ComboBox"),
            w!(""),
            WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | WS_TABSTOP.0),
            scale(x),
            scale(y),
            scale(w),
            scale(h),
            Some(parent),
            Some(HMENU((id as isize) as *mut c_void)),
            Some(hinst),
            None,
        )?
    };

    for (i, item) in items.iter().enumerate() {
        let wide_item: Vec<u16> = item.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            SendMessageW(
                hwnd,
                CB_ADDSTRING,
                Some(WPARAM(0)),
                Some(LPARAM(wide_item.as_ptr() as isize)),
            )
        };
        if let Some(sel) = selected {
            if i == sel {
                unsafe { SendMessageW(hwnd, CB_SETCURSEL, Some(WPARAM(i)), Some(LPARAM(0))) };
            }
        }
    }
    Ok(hwnd)
}

// Dedicated window procedure for the settings dialog. It MUST NOT share the
// screen-saver `wnd_proc`, whose mouse/key handling destroys the window: that
// would make the dialog dismiss itself the moment the pointer moves or a
// button is clicked (the reported "flash and exit" crash).
extern "system" fn dlg_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_INITDIALOG => LRESULT(1), // We handle creation manually in show_settings_dialog
        WM_COMMAND => {
            let ctrl_id = (wparam.0 as u32 & 0xffff) as i32;
            match ctrl_id {
                IDC_OK => {
                    // Capture all settings before closing
                    capture_and_save_settings(hwnd);
                    if let Some(c) = DIALOG_CONFIRMED.get() {
                        *c.lock().unwrap() = true;
                    }
                    unsafe { DestroyWindow(hwnd) };
                }
                IDC_CANCEL => {
                    if let Some(c) = DIALOG_CONFIRMED.get() {
                        *c.lock().unwrap() = false;
                    }
                    unsafe { DestroyWindow(hwnd) };
                }
                _ => {} // Control notifications — ignore.
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // Treat X button as Cancel.
            if let Some(c) = DIALOG_CONFIRMED.get() {
                *c.lock().unwrap() = false;
            }
            unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn register_dlg_class(hinstance: HINSTANCE) -> anyhow::Result<()> {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(dlg_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        // Win11: use slightly off-white background
        hbrBackground: unsafe { CreateSolidBrush(COLORREF(0xF3F3F3)) },
        lpszClassName: CLASS_NAME_DLG,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };
    Ok(())
}

fn show_settings_dialog() -> anyhow::Result<()> {
    let settings = Settings::load();
    let hinstance = register_class()?;
    register_dlg_class(hinstance)?;

    // Initialise the global result slots used by dlg_proc.
    DIALOG_RESULT.get_or_init(|| std::sync::Mutex::new(None));
    DIALOG_CONFIRMED.get_or_init(|| std::sync::Mutex::new(false));

    // Win11 style dialog: compact card layout, modern proportions.
    // Base size in pixels (scaled by DPI at runtime).
    let dlg_width = 360;
    let dlg_height = 320;

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
            CLASS_NAME_DLG,
            w!("Tank Matrix Saver Settings"),
            WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            dlg_width,
            dlg_height,
            None,
            None,
            Some(hinstance),
            None,
        )?
    };

    // DPI-aware scaling.
    let hdc = unsafe { GetDC(Some(hwnd)) };
    let dpi = if !hdc.is_invalid() {
        let dpi_val = unsafe { GetDeviceCaps(Some(hdc), LOGPIXELSX) };
        unsafe { ReleaseDC(Some(hwnd), hdc) };
        dpi_val as f32
    } else {
        96.0
    };
    *DPI_SCALE.lock().unwrap() = dpi / 96.0;

    // Resize window based on DPI.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            (dlg_width as f32 * dpi / 96.0) as i32,
            (dlg_height as f32 * dpi / 96.0) as i32,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };

    let hinst = HINSTANCE(unsafe { GetWindowLongPtrW(hwnd, GWLP_HINSTANCE) } as *mut c_void);

    let margin_x = 24;
    let content_w = dlg_width - 2 * margin_x;

    // ---- Style (version) ----
    create_label(hwnd, hinst, "Style", margin_x, 18, content_w, 20)?;
    // Combo entries show the bare style name (no "version:" prefix).
    let entries: Vec<String> = VERSIONS.iter().map(|v| v.to_string()).collect();
    let selected = VERSIONS.iter().position(|v| *v == settings.version);
    create_combobox(
        hwnd,
        hinst,
        IDC_VERSION_COMBO,
        margin_x,
        42,
        content_w,
        200,
        &entries.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        selected,
    )?;

    // ---- Options ----
    create_label(hwnd, hinst, "Options", margin_x, 78, content_w, 20)?;
    create_checkbox(
        hwnd,
        hinst,
        IDC_MIRROR,
        "Mirror effect",
        margin_x,
        104,
        content_w,
        24,
        settings.mirror,
    )?;
    create_checkbox(
        hwnd,
        hinst,
        IDC_SKIP_INTRO,
        "Skip intro animation",
        margin_x,
        134,
        content_w,
        24,
        settings.skip_intro,
    )?;

    // ---- Buttons (bottom row) ----
    let btn_w = 90;
    let btn_h = 30;
    let btn_y = 178;
    create_push_button(
        hwnd,
        hinst,
        IDC_CANCEL,
        "Cancel",
        scale(margin_x + content_w - btn_w),
        scale(btn_y),
        scale(btn_w),
        scale(btn_h),
        false,
    )?;
    create_push_button(
        hwnd,
        hinst,
        IDC_OK,
        "OK",
        scale(margin_x + content_w - btn_w - btn_w - 10),
        scale(btn_y),
        scale(btn_w),
        scale(btn_h),
        true,
    )?;

    // Center on screen and show.
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect);
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let dlg_w = rect.right - rect.left;
        let dlg_h = rect.bottom - rect.top;
        SetWindowPos(
            hwnd,
            None,
            (screen_w - dlg_w) / 2,
            (screen_h - dlg_h) / 2,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        ShowWindow(hwnd, SW_SHOW);
    }

    // Modal message loop; OK saves / Cancel or X discards. Logic lives in dlg_proc.
    let mut msg = MSG::default();
    while bool::from(unsafe { GetMessageW(&mut msg, None, 0, 0) }) {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

fn parse_hwnd(arg: &str) -> Option<HWND> {
    let arg = arg.trim_start_matches("0x").trim_start_matches("0X");
    let value = u64::from_str_radix(arg, 16)
        .or_else(|_| arg.parse::<u64>())
        .ok()?;
    let nz = std::num::NonZeroIsize::new(value as isize)?;
    Some(HWND(nz.get() as *mut c_void))
}

fn main() -> anyhow::Result<()> {
    // Hide console window for seamless experience (no flash of console)
    #[cfg(windows)]
    unsafe {
        // Use FreeConsole to detach from console - this prevents the flash
        // We need to use the raw API since the feature might not be enabled
        let kernel32 = GetModuleHandleW(w!("kernel32.dll")).unwrap_or_default();
        if !kernel32.is_invalid() {
            type FnFreeConsole = extern "system" fn() -> i32;
            let proc = windows::Win32::System::LibraryLoader::GetProcAddress(
                kernel32,
                windows::core::s!("FreeConsole"),
            );
            if !proc.is_none() {
                let free_console: FnFreeConsole = std::mem::transmute_copy(&proc);
                let _ = free_console();
            }
        }
    }

    // Debug: capture panics to a log file (the Windows screen saver host has no
    // console, so any renderer panic would otherwise be silently swallowed).
    let _ = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("tank_saver_panic.log")
        {
            use std::io::Write;
            let _ = writeln!(f, "PANIC: {info}");
        }
    }));

    let args: Vec<String> = std::env::args().collect();
    let settings = Settings::load();

    // The first argument is the program path; the switch is the second one.
    let switch = args
        .get(1)
        .map(|a| a.to_lowercase())
        .unwrap_or_default();

    if switch == "/c" || switch.starts_with("/c:") {
        show_settings_dialog()
    } else if switch == "/p" {
        let hwnd = args
            .get(2)
            .and_then(|a| parse_hwnd(a))
            .unwrap_or(HWND(std::ptr::null_mut()));
        if hwnd.is_invalid() {
            return Ok(());
        }
        run_preview(hwnd, settings)
    } else if let Some(rest) = switch.strip_prefix("/p:") {
        let hwnd = parse_hwnd(rest).unwrap_or(HWND(std::ptr::null_mut()));
        if hwnd.is_invalid() {
            return Ok(());
        }
        run_preview(hwnd, settings)
    } else {
        // /s or no argument -> run full screen.
        run_fullscreen(settings)
    }
}
