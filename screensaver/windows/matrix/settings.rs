// Native Windows storage and resource-backed settings dialog.

use std::{ffi::c_void, io};

use windows::{
    core::{w, Error, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            DialogBoxParamW, EndDialog, MessageBoxW, SendDlgItemMessageW, BM_GETCHECK, BM_SETCHECK,
            CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, IDCANCEL, IDOK, MB_ICONERROR, MB_OK,
            WM_COMMAND, WM_INITDIALOG,
        },
    },
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

use super::SaverSettings;

const SETTINGS_DIALOG_ID: u16 = 101;
const VERSION_COMBO_ID: i32 = 1001;
const MIRROR_CHECKBOX_ID: i32 = 1002;
const SKIP_INTRO_CHECKBOX_ID: i32 = 1003;
const REGISTRY_PATH: &str = r"Software\Arctan95\Matrix";

const VERSIONS: &[(&str, &str)] = &[
    ("classic", "Classic"),
    ("3d", "3D"),
    ("neomatrixology", "Neomatrixology"),
    ("megacity", "Megacity"),
    ("operator", "Operator"),
    ("resurrections", "Resurrections"),
    ("paradise", "Paradise"),
    ("nightmare", "Nightmare"),
    ("trinity", "Trinity"),
    ("morpheus", "Morpheus"),
    ("bugs", "Bugs"),
];

pub(super) fn load() -> SaverSettings {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = current_user.open_subkey(REGISTRY_PATH) else {
        return SaverSettings::new("classic", false, false);
    };

    let version = key
        .get_value::<String, _>("Version")
        .unwrap_or_else(|_| "classic".to_owned());
    let version = VERSIONS
        .iter()
        .find(|(id, _)| *id == version)
        .map(|(id, _)| *id)
        .unwrap_or("classic");
    let mirror_enabled = key.get_value::<u32, _>("MirrorEnabled").unwrap_or(0) != 0;
    let skip_intro = key.get_value::<u32, _>("SkipIntro").unwrap_or(0) != 0;

    SaverSettings::new(version, mirror_enabled, skip_intro)
}

fn save(settings: &SaverSettings) -> io::Result<()> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = current_user.create_subkey(REGISTRY_PATH)?;
    key.set_value("Version", &settings.version)?;
    key.set_value("MirrorEnabled", &u32::from(settings.mirror_enabled))?;
    key.set_value("SkipIntro", &u32::from(settings.skip_intro))?;
    Ok(())
}

pub(super) fn show(owner: Option<isize>) -> anyhow::Result<()> {
    unsafe {
        let module = GetModuleHandleW(None)?;
        let instance = HINSTANCE(module.0);
        let owner = owner.map(|value| HWND(value as *mut c_void));
        let resource = PCWSTR(SETTINGS_DIALOG_ID as usize as *const u16);
        let result = DialogBoxParamW(
            Some(instance),
            resource,
            owner,
            Some(dialog_proc),
            LPARAM(0),
        );
        if result == -1 {
            return Err(Error::from_thread().into());
        }
    }
    Ok(())
}

unsafe extern "system" fn dialog_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> isize {
    match message {
        WM_INITDIALOG => {
            initialize_dialog(window);
            1
        }
        WM_COMMAND => {
            let control_id = (wparam.0 & 0xffff) as i32;
            if control_id == IDOK.0 {
                match save(&read_dialog_settings(window)) {
                    Ok(()) => {
                        let _ = EndDialog(window, IDOK.0 as isize);
                    }
                    Err(error) => show_error(Some(window), &error.to_string()),
                }
                1
            } else if control_id == IDCANCEL.0 {
                let _ = EndDialog(window, IDCANCEL.0 as isize);
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

unsafe fn initialize_dialog(window: HWND) {
    let settings = load();
    for (_, title) in VERSIONS {
        let title = wide(title);
        SendDlgItemMessageW(
            window,
            VERSION_COMBO_ID,
            CB_ADDSTRING,
            WPARAM(0),
            LPARAM(title.as_ptr() as isize),
        );
    }

    let version_index = VERSIONS
        .iter()
        .position(|(id, _)| *id == settings.version)
        .unwrap_or_default();
    SendDlgItemMessageW(
        window,
        VERSION_COMBO_ID,
        CB_SETCURSEL,
        WPARAM(version_index),
        LPARAM(0),
    );
    SendDlgItemMessageW(
        window,
        MIRROR_CHECKBOX_ID,
        BM_SETCHECK,
        WPARAM(usize::from(settings.mirror_enabled)),
        LPARAM(0),
    );
    SendDlgItemMessageW(
        window,
        SKIP_INTRO_CHECKBOX_ID,
        BM_SETCHECK,
        WPARAM(usize::from(settings.skip_intro)),
        LPARAM(0),
    );
}

unsafe fn read_dialog_settings(window: HWND) -> SaverSettings {
    let version_index =
        SendDlgItemMessageW(window, VERSION_COMBO_ID, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0
            as usize;
    let version = VERSIONS
        .get(version_index)
        .map(|(id, _)| *id)
        .unwrap_or("classic");
    let mirror_enabled = SendDlgItemMessageW(
        window,
        MIRROR_CHECKBOX_ID,
        BM_GETCHECK,
        WPARAM(0),
        LPARAM(0),
    )
    .0 == 1;
    let skip_intro = SendDlgItemMessageW(
        window,
        SKIP_INTRO_CHECKBOX_ID,
        BM_GETCHECK,
        WPARAM(0),
        LPARAM(0),
    )
    .0 == 1;

    SaverSettings::new(version, mirror_enabled, skip_intro)
}

fn show_error(owner: Option<HWND>, message: &str) {
    let message = wide(message);
    unsafe {
        MessageBoxW(
            owner,
            PCWSTR(message.as_ptr()),
            w!("Matrix"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
