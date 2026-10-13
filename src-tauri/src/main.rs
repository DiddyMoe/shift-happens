#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!("Shift Happens is a Windows-only desktop application. Build on Windows.");

fn main() {
    shift_happens_lib::run();
}
