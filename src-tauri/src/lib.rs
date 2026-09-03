mod base;
mod config;
mod electron;
mod features;
mod hw;
mod install;
mod launch;
mod status;
mod system;
mod updates;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Launcher GPU preference — mirrors Electron early switch (main.js 323-350)
    // Reads desktop-config.json before WebView2 creation and sets browser args.
    // Supports: auto | integrated (low-power) | dedicated (high-perf) | disabled (SwiftShader)
    {
        let cfg_path = crate::base::get_config_file();
        if cfg_path.exists() {
            if let Ok(s) = std::fs::read_to_string(&cfg_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    let lg = v.get("launcherGpu")
                        .and_then(|x| x.as_str())
                        .unwrap_or_else(|| if v.get("electronGpu").and_then(serde_json::Value::as_bool) == Some(false) { "disabled" } else { "auto" })
                        .trim().to_string();
                    match lg.as_str() {
                        "disabled" => std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", "--disable-gpu"),
                        "integrated" => std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", "--force_low_power_gpu"),
                        "dedicated" => std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", "--force_high_performance_gpu"),
                        _ => {}
                    }
                    eprintln!("[launcher] GPU preference: {} — {}", lg, match lg.as_str() { "integrated" => "iGPU (power saving, frees VRAM)", "dedicated" => "dGPU (high perf)", "disabled" => "SwiftShader (max VRAM)", _ => "OS decides" });
                }
            }
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            system::greet, hw::detect_gpu, hw::detect_gpus, hw::detect_hardware, hw::get_hardware_profile, hw::get_system_metrics,
            status::get_status, status::check_python, status::check_git, status::check_installed, status::check_command,
            config::config_load, config::config_save, config::get_install_paths, config::get_disk_space, config::get_model_paths, config::detect_model_folders,
            config::install_plan, config::validate_install, config::uv_cache_info, config::uv_cache_size, config::manage_list,
            updates::get_desktop_version, updates::get_wangp_local_version, updates::get_desktop_git_info,
            launch::launch, launch::stop_wangp, system::open_folder, system::select_folder, system::confirm_dialog, system::repair_settings,
            features::check_package, features::check_package_updates, features::deepy_status, features::memory_profile_read,
            features::auto_tune_detect, features::auto_tune_recommend,
            install::install, install::reinstall, install::uninstall, install::sync_kernels, install::update, config::manage_set_active, config::uninstall_env,
            launch::open_external, launch::detect_browsers, launch::launch_browser, launch::launch_browser_no_gpu, launch::chrome_available,
            system::set_data_dir, system::reset_data_dir, system::migrate_to_preferred, system::move_folder, system::write_wgp_config, install::install_prerequisite,
            updates::get_wangp_upstream_info, updates::get_wangp_version, system::report_issue, system::create_desktop_shortcut, electron::detect_electron, electron::uninstall_electron,
            features::upgrade_package, features::install_package, features::uninstall_package, features::restore_requirements,
            features::llm_engines_list, features::llm_engine_install, features::llm_engine_serve, features::llm_engine_auth,
            features::deepy_activate, features::deepy_set, features::set_auto_start, features::memory_profile_apply,
            features::notifier_config, features::notifier_set, features::notifier_test, features::pulsebar_hide, features::pulsebar_show, features::pulsebar_state,
            features::set_theme_follow_system, features::set_notifications_enabled,
            updates::check_update, updates::download_update, updates::install_update,
            system::create_browser_view, system::destroy_browser_view, system::get_log_history, config::uv_cache_clean,
            system::open_task_manager, system::get_crash_recovery_info,
            launch::launch_webview, launch::popout_webview, system::hide_browser_view, system::detach_browser_view, system::reattach_browser_view,
            system::create_term_view, system::destroy_term_view, system::bv_navigate, system::bv_set_zoom, system::bv_set_dock,
            system::is_data_dir_roaming, system::migrate_choose, system::notifier_ensure, system::ui_mode_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
