// Hide the console window in release builds; keep it in debug for `tauri dev` logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wan2gp_tauri_spike_lib::run();
}
