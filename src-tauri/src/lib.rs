mod amd;
mod base;
mod cleanup;
mod config;
mod deepy_web;
mod electron;
mod features;
mod hw;
mod install;
mod launch;
mod log_server;
mod notifier_native;
mod platform;
mod plugins;
mod resolutions;
mod status;
mod system;
mod troubleshoot;
mod updates;
mod upstream_health;
mod wheels;

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
/// Ordered app-close state: first X starts session shutdown (window stays),
/// worker closes the window when sessions are dead; second X forces out.
static APP_CLOSING: AtomicBool = AtomicBool::new(false);
static CLOSE_CONFIRMED: AtomicBool = AtomicBool::new(false);

/// Chromium flag that carries the saved launcher-GPU preference into the
/// WebView2 runtime. `None` = let the OS decide.
///
/// `integrated` must never map to `--disable-gpu`: that tears down the GPU
/// process and with it the platform (Media Foundation) decoder, so
/// `canPlayType("hev1"/"hvc1")` turns into "no" and every HEVC clip in the
/// gallery plays sound over a black frame while H.264 keeps working. Measured
/// on WebView2 155: `--disable-gpu` → hev1/hvc1 "no"; `--force_low_power_gpu`
/// and no-flag → "probably" (h264/av1/vp9 stay "probably" in all three).
/// `--force_low_power_gpu` only *asks* for the low-power adapter, so a runtime
/// that ignores it still ends up on the OS default — the iGPU on the
/// switchable-graphics laptops this setting exists for.
pub(crate) fn launcher_gpu_flag(pref: &str) -> Option<&'static str> {
    match pref {
        // SwiftShader: frees the dGPU but loses HEVC previews.
        "disabled" => Some("--disable-gpu"),
        "integrated" => Some("--force_low_power_gpu"),
        "dedicated" => Some("--force_high_performance_gpu"),
        _ => None,
    }
}

/// Saved launcher-GPU preference (`auto|integrated|dedicated|disabled`).
/// `electronGpu: false` in an old config means the retired Electron launcher's
/// "run the UI without the GPU", so it maps to `disabled` — the same meaning.
/// Pure apart from the file read.
pub(crate) fn launcher_gpu_pref_from(v: &serde_json::Value) -> String {
    v.get("launcherGpu")
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| {
            if v.get("electronGpu").and_then(serde_json::Value::as_bool) == Some(false) {
                "disabled"
            } else {
                "auto"
            }
        })
        .trim()
        .to_lowercase()
}

/// The preference saved in desktop-config.json (`auto` when unreadable).
pub(crate) fn launcher_gpu_pref() -> String {
    let cfg_path = crate::base::get_config_file();
    match std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    {
        Some(v) => launcher_gpu_pref_from(&v),
        None => "auto".into(),
    }
}

/// One line for the debug bundle: `integrated (--force_low_power_gpu)`.
pub(crate) fn launcher_gpu_summary() -> String {
    let pref = launcher_gpu_pref();
    match launcher_gpu_flag(&pref) {
        Some(f) => format!("{pref} ({f})"),
        None => format!("{pref} (no WebView2 flag)"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Crash reporting first, before anything else can crash: the release profile
    // is `panic = "abort"`, so the hook is the only chance to leave a trace, and
    // a crash from a previous session is surfaced in the console history.
    crate::base::install_panic_hook();
    if let Some(line) = crate::base::report_previous_crash() {
        crate::base::push_log(&line, "launcher");
    }
    // Launcher GPU preference — mirrors Electron early switch (main.js 323-350)
    // Reads desktop-config.json before WebView2 creation and sets browser args.
    // Supports: auto | integrated (low-power) | dedicated (high-perf) | disabled (SwiftShader)
    {
        let cfg_path = crate::base::get_config_file();
        if cfg_path.exists() {
            if let Ok(s) = std::fs::read_to_string(&cfg_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    let lg = launcher_gpu_pref_from(&v);
                    // Documented WebView2 variable (the previous
                    // ..._BROWSER_ARGS spelling does not exist, so this whole
                    // setting silently did nothing). Append, never replace:
                    // the runtime may already carry flags.
                    //
                    let flag = launcher_gpu_flag(&lg);
                    if let Some(flag) = flag {
                        const KEY: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
                        let existing = std::env::var(KEY).unwrap_or_default();
                        if !existing.split_whitespace().any(|a| a == flag) {
                            let merged = if existing.trim().is_empty() {
                                flag.to_string()
                            } else {
                                format!("{} {flag}", existing.trim())
                            };
                            std::env::set_var(KEY, merged);
                        }
                    }
                    eprintln!(
                        "[launcher] GPU preference: {} — {} ({})",
                        lg,
                        match lg.as_str() {
                            "integrated" => "iGPU (power saving, keeps video decode)",
                            "dedicated" => "dGPU (high perf)",
                            "disabled" => "SwiftShader (max VRAM, no HEVC previews)",
                            _ => "OS decides",
                        },
                        flag.unwrap_or("no WebView2 flag")
                    );
                    // Support sees the effective flag in the console history and
                    // the troubleshoot bundle instead of guessing from the config.
                    crate::base::push_log(
                        &format!(
                            "[launcher] launcher GPU: {lg} — WebView2: {}",
                            flag.unwrap_or("(none, OS decides)")
                        ),
                        "launcher",
                    );
                }
            }
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Clear any terminal script / staged token file a crashed previous
            // session left in %TEMP% (tokens are no longer written to the .bat,
            // but older builds did).
            crate::launch::sweep_terminal_temp_files();
            // Desktop window mode (issue #63): config ships the main window
            // hidden and pre-maximized (the 0.2.0 default), setup re-applies
            // the saved desktopWindowMode invisibly, then shows the window —
            // no maximized→windowed flash on launch, whatever the mode is.
            let mode = crate::base::load_config_value()
                .get("desktopWindowMode")
                .and_then(|v| v.as_str())
                .unwrap_or("maximized")
                .to_string();
            let mode = crate::features::window_mode_normalize(&mode);
            if let Some(w) = app.get_webview_window("main") {
                crate::features::apply_window_mode(&w, &mode);
                let _ = w.show();
                let _ = w.set_focus();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Ordered close: sessions (running AND old/orphaned) die FIRST,
            // the app exits SECOND. Fast kills fire synchronously (ms),
            // then the window stays alive while a worker runs the full
            // verified sweep (WMI + port scans catch orphans the fast path
            // can't see) and closes the window when done. Second X or a
            // 30s watchdog forces the exit so nobody gets trapped.
            // Explorer/browser windows are never touched.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if CLOSE_CONFIRMED.load(Ordering::SeqCst) {
                        return; // worker/watchdog approved — proceed with close
                    }
                    if APP_CLOSING.swap(true, Ordering::SeqCst) {
                        // Second X while sessions are stopping: force out now.
                        CLOSE_CONFIRMED.store(true, Ordering::SeqCst);
                        window.app_handle().exit(0);
                        return;
                    }
                    api.prevent_close();
                    crate::system::shutdown_cleanup_fast();
                    {
                        use tauri::Emitter;
                        let _ = window.emit("app-closing", serde_json::json!({ "stopping": true }));
                    }
                    let app = window.app_handle().clone();
                    std::thread::spawn(move || {
                        use tauri::Manager;
                        crate::system::shutdown_cleanup_full(&app);
                        CLOSE_CONFIRMED.store(true, Ordering::SeqCst);
                        if let Some(m) = app.get_window("main") {
                            let _ = m.close();
                        } else {
                            app.exit(0);
                        }
                    });
                    let app2 = window.app_handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(30));
                        if !CLOSE_CONFIRMED.load(Ordering::SeqCst) {
                            CLOSE_CONFIRMED.store(true, Ordering::SeqCst);
                            app2.exit(0);
                        }
                    });
                }
            }
            // Term console window closed via X: tell the main window so its
            // console-visible flag doesn't go stale (else Manage-close would
            // resurrect the console).
            if window.label() == "term" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    use tauri::Emitter;
                    if let Some(m) = window.app_handle().get_window("main") {
                        let _ = m.emit("term-closed", serde_json::json!({"closed": true}));
                    }
                }
            }
        })
        // Command threading: Tauri runs a command on the thread that invokes it
        // unless it is `async`, so anything heavy and user-initiated is declared
        // `#[tauri::command(async)]` at its definition (repair_settings,
        // report_issue, troubleshoot_debug_bundle, detect_electron, the library
        // scans). The frequently-polled ones (get_status,
        // get_system_metrics) stay sync on purpose: they are cache-throttled,
        // and keeping their timing unchanged matters more than the move.
        .invoke_handler(tauri::generate_handler![
            hw::detect_gpu,
            hw::detect_gpus,
            hw::detect_hardware,
            hw::get_hardware_profile,
            hw::get_system_metrics,
            status::get_status,
            status::check_installed,
            status::check_command,
            config::config_load,
            config::config_save,
            config::get_install_paths,
            config::get_disk_space,
            config::get_model_paths,
            config::detect_model_folders,
            config::install_plan,
            config::validate_install,
            config::manage_list,
            config::library_loras,
            config::library_models,
            config::library_model_delete,
            config::library_finetunes,
            config::library_finetune_import,
            config::library_finetune_delete,
            config::library_finetune_content,
            resolutions::custom_resolutions_read,
            resolutions::custom_resolutions_write,
            config::workspace_list,
            config::workspace_protect,
            config::workspace_backup,
            updates::get_desktop_version,
            updates::get_wangp_local_version,
            updates::get_desktop_git_info,
            launch::launch,
            launch::stop_wangp,
            launch::stop_all_servers,
            system::open_folder,
            system::select_folder,
            system::confirm_dialog,
            system::repair_settings,
            system::reset_wgp_config,
            features::check_package,
            features::check_package_updates,
            features::deepy_status,
            features::memory_profile_read,
            features::auto_tune_detect,
            features::auto_tune_recommend,
            install::install,
            install::reinstall,
            install::uninstall,
            install::sync_kernels,
            install::restore_kernels,
            install::install_hip_gguf_wheel,
            install::install_wgpkernels_wheel,
            install::update,
            install::dep_check,
            install::dlss5_status,
            install::triton_runtime_status,
            install::install_dlss5,
            install::dlss5_check,
            install::dlss5_update_check,
            install::dlss5_verify_files,
            install::dlss5_cached_state,
            install::dlss5_apply_workstation_fix,
            install::dlss5_revert_workstation_fix,
            install::classify_target,
            install::python_preflight,
            install::restore_backup,
            install::verify_wangp_files,
            install::repair_wangp_files,
            install::rollback_wangp,
            config::manage_set_active,
            config::uninstall_env,
            launch::detect_browsers,
            launch::launch_browser,
            launch::launch_browser_no_gpu,
            launch::no_gpu_available,
            launch::chrome_available,
            system::set_data_dir,
            system::reset_data_dir,
            system::migrate_to_preferred,
            system::move_folder,
            system::folder_size,
            system::downloads_since,
            system::save_downloaded_file,
            system::write_wgp_config,
            install::install_prerequisite,
            install::prerequisite_report,
            updates::get_wangp_upstream_info,
            updates::wangp_contains_commit,
            updates::get_wangp_version,
            system::report_issue,
            system::config_backups_list,
            system::config_backup_restore,
            system::upstream_changelog,
            system::create_desktop_shortcut,
            electron::detect_electron,
            electron::uninstall_electron,
            features::upgrade_package,
            features::install_package,
            features::uninstall_package,
            features::restore_requirements,
            features::llm_engines_list,
            features::llm_engine_install,
            features::llm_engine_uninstall,
            features::llm_engine_serve,
            features::llm_engine_auth,
            features::deepy_activate,
            features::deepy_set,
            features::set_auto_start,
            features::memory_profile_apply,
            features::notifier_config,
            features::notifier_set,
            features::notifier_test,
            notifier_native::notifier_native_status,
            notifier_native::notifier_native_load,
            notifier_native::notifier_native_save,
            notifier_native::notifier_native_test,
            plugins::plugins_disable_third_party,
            features::set_theme_follow_system,
            features::set_notifications_enabled,
            features::set_window_mode,
            features::cursor_info,
            updates::check_update,
            updates::download_update,
            updates::install_update,
            updates::launcher_release_info,
            updates::relaunch_app,
            system::create_browser_view,
            system::destroy_browser_view,
            system::get_log_history,
            cleanup::cleanup_scan,
            cleanup::cleanup_clean,
            system::webview_memory,
            system::bv_sync_bounds,
            system::save_staged_download,
            system::toggle_term_window,
            system::term_set_dock,
            system::export_logs,
            system::mirror_console,
            system::clear_log_history,
            system::open_task_manager,
            system::get_crash_recovery_info,
            launch::launch_webview,
            launch::batch_process,
            system::hide_browser_view,
            system::detach_browser_view,
            system::reattach_browser_view,
            system::create_term_view,
            system::request_console_seed,
            system::term_console_seed,
            system::destroy_term_view,
            system::bv_navigate,
            system::bv_set_zoom,
            system::bv_set_dock,
            system::is_data_dir_roaming,
            system::migrate_choose,
            system::notifier_ensure,
            system::ui_mode_set,
            troubleshoot::troubleshoot_failsafe_apply,
            troubleshoot::troubleshoot_cuda_check,
            troubleshoot::troubleshoot_port_status,
            troubleshoot::troubleshoot_port_fix,
            troubleshoot::troubleshoot_long_paths_status,
            troubleshoot::troubleshoot_long_paths_enable,
            troubleshoot::troubleshoot_debug_bundle,
            troubleshoot::troubleshoot_triton_test,
            troubleshoot::troubleshoot_triton_clear,
            troubleshoot::troubleshoot_msvc_status,
            troubleshoot::troubleshoot_msvc_install,
            troubleshoot::troubleshoot_triton_repair,
            launch::set_triton_compiler,
            troubleshoot::troubleshoot_gpu_compute,
            troubleshoot::troubleshoot_known_good,
            troubleshoot::troubleshoot_oom_remedy,
            troubleshoot::troubleshoot_vram_diag,
            troubleshoot::troubleshoot_flag_audit,
            troubleshoot::troubleshoot_model_integrity,
            troubleshoot::troubleshoot_yue2_metadata,
            troubleshoot::troubleshoot_yue2_quarantine,
            deepy_web::deepy_web_preflight,
            deepy_web::deepy_web_start,
            deepy_web::deepy_web_stop,
            deepy_web::deepy_web_status,
            deepy_web::deepy_web_cert,
            deepy_web::deepy_web_tailscale,
            deepy_web::deepy_web_open_outputs,
            deepy_web::main_lan_urls,
            log_server::log_server_start,
            log_server::log_server_stop,
            log_server::log_server_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod launcher_gpu_tests {
    use super::launcher_gpu_flag;

    #[test]
    fn integrated_never_disables_the_gpu_process() {
        // Regression: v0.10.23 mapped this to --disable-gpu, which removed the
        // platform HEVC decoder — the gallery played HEVC as audio over black.
        assert_ne!(launcher_gpu_flag("integrated"), Some("--disable-gpu"));
        assert_eq!(
            launcher_gpu_flag("integrated"),
            Some("--force_low_power_gpu")
        );
    }

    #[test]
    fn remaining_preferences_keep_their_flags() {
        assert_eq!(launcher_gpu_flag("disabled"), Some("--disable-gpu"));
        assert_eq!(
            launcher_gpu_flag("dedicated"),
            Some("--force_high_performance_gpu")
        );
        assert_eq!(launcher_gpu_flag("auto"), None);
        assert_eq!(launcher_gpu_flag(""), None);
        assert_eq!(launcher_gpu_flag("nonsense"), None);
    }

    #[test]
    fn preference_is_read_from_every_config_spelling() {
        use super::launcher_gpu_pref_from;
        assert_eq!(
            launcher_gpu_pref_from(&serde_json::json!({"launcherGpu": " Integrated "})),
            "integrated"
        );
        // Retired Electron launcher's "no GPU" toggle means SwiftShader.
        assert_eq!(
            launcher_gpu_pref_from(&serde_json::json!({"electronGpu": false})),
            "disabled"
        );
        assert_eq!(
            launcher_gpu_pref_from(&serde_json::json!({"electronGpu": true})),
            "auto"
        );
        assert_eq!(launcher_gpu_pref_from(&serde_json::json!({})), "auto");
    }
}
