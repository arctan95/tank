// Windows .scr executable entry point.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn main() {
    tank::run_windows_saver();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("matrix-saver can only run on Windows");
}
