// ponytail: windows_subsystem="windows" removed — was hiding console only in release, causing debug `cargo tauri dev` to open/close console window (flicker)
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wan2gp_tauri_spike_lib::run();
}
