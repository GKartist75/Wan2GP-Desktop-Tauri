// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// ponytail: std::process only — no shell plugin needed for read-only probes
#[tauri::command]
fn detect_gpu() -> Result<serde_json::Value, String> {
    use std::process::Command;
    // Try nvidia-smi first (mirrors main.js getGpuInfo)
    if let Ok(out) = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let parts: Vec<&str> = s.split(", ").collect();
                return Ok(serde_json::json!({
                    "vendor": "NVIDIA",
                    "name": parts.get(0).unwrap_or(&"").to_string(),
                    "vramMB": parts.get(1).unwrap_or(&"0 MiB").to_string(),
                    "driverVersion": parts.get(2).unwrap_or(&"").trim().to_string(),
                    "raw": s
                }));
            }
        }
    }
    // fallback: no NVIDIA
    Ok(serde_json::json!({
        "vendor": "unknown",
        "name": "",
        "vramMB": "0",
        "driverVersion": "",
        "raw": "nvidia-smi not found or no NVIDIA GPU"
    }))
}

#[tauri::command]
fn get_status() -> serde_json::Value {
    serde_json::json!({
        "spike": true,
        "message": "This is the Tauri spike — real wan2gp-desktop has 100 handlers here. This one proves invoke() replaces ipcRenderer.invoke().",
        "electron_equivalent": "ipcMain.handle('get-status', ...)"
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, detect_gpu, get_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
