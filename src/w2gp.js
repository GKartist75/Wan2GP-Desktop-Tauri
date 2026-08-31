// ponytail: 1-file compat shim — maps Electron preload window.w2gp.* to Tauri invoke snake_case
// so reference/renderer/app.js needs only `window.w2gp = w2gp` and works. No new dep.
const inv = (name, args) => window.__TAURI__.core.invoke(name, args);

function call(name, args) {
  // Tauri commands are snake_case; Electron used kebab/colon. Normalize.
  const key = name.replace(/[-:]/g, '_');
  return inv(key, args);
}

export const w2gp = {
  platform: navigator.platform.includes('Win') ? 'win32' : navigator.platform.includes('Mac') ? 'darwin' : 'linux',
  // A
  checkInstalled: () => call('check_installed'), detectGpu: () => call('detect_gpu'), detectGpus: () => call('detect_gpus'),
  detectHardware: () => call('detect_hardware'), getHardwareProfile: () => call('get_hardware_profile'), getSystemMetrics: () => call('get_system_metrics'),
  autoTuneDetect: () => call('auto_tune_detect'), autoTuneRecommend: (hw, opts) => call('auto_tune_recommend', { hw, opts }),
  // B
  install: (envType) => call('install', { envType }), reinstall: () => call('reinstall'), uninstall: () => call('uninstall'),
  syncKernels: () => call('sync_kernels'), installPlan: () => call('install_plan'), validateInstall: () => call('validate_install'),
  manageList: () => call('manage_list'), manageSetActive: (name) => call('manage_set_active', { name }), uninstallEnv: (name) => call('uninstall_env', { name }),
  uvCacheInfo: () => call('uv_cache_info'), uvCacheSize: () => call('uv_cache_size'), uvCacheClean: (a) => call('uv_cache_clean', { action: a }),
  checkCommand: (cmd) => call('check_command', { cmd }), installPrerequisite: (tool) => call('install_prerequisite', { tool }),
  // C
  getInstallPaths: () => call('get_install_paths'), getDiskSpace: (path) => call('get_disk_space', { path }),
  openFolder: (p) => call('open_folder', { path: p }), setDataDir: (dir) => call('set_data_dir', { dir }),
  resetDataDir: () => call('reset_data_dir'), migrateToPreferred: (c) => call('migrate_to_preferred', { choices: c }),
  moveFolder: (src, dst) => call('move_folder', { src, dst }), migrateChoose: () => call('migrate_choose'),
  isDataDirRoaming: () => call('is_data_dir_roaming'),
  writeWgpConfig: (cfg) => call('write_wgp_config', { cfg }), selectFolder: () => call('select_folder'),
  confirmDialog: (opts) => call('confirm_dialog', { opts }), detectModelFolders: () => call('detect_model_folders'),
  getModelPaths: () => call('get_model_paths'), repairSettings: () => call('repair_settings'),
  // D
  getStatus: () => call('get_status'), launch: (mode) => call('launch', { mode }), launchWebview: () => call('launch_webview'),
  stopWangp: () => call('stop_wangp'), popoutWebview: (url) => call('popout_webview', { url }),
  createBrowserView: (url, opts) => call('create_browser_view', { url, opts }),
  hideBrowserView: () => call('hide_browser_view'), destroyBrowserView: () => call('destroy_browser_view'),
  detachBrowserView: () => call('detach_browser_view'), reattachBrowserView: () => call('reattach_browser_view'),
  createTermView: () => call('create_term_view'), destroyTermView: () => call('destroy_term_view'),
  bvNavigate: (a) => call('bv_navigate', { action: a }), bvSetZoom: (f) => call('bv_set_zoom', { factor: f }), bvSetDock: (d) => call('bv_set_dock', { dock: d }),
  getLogHistory: () => call('get_log_history'),
  // E/F/G/H/I/J
  openExternal: (url) => call('open_external', { url }), detectBrowsers: () => call('detect_browsers'),
  launchBrowser: (url) => call('launch_browser', { url }), launchBrowserNoGpu: (url) => call('launch_browser_no_gpu', { url }),
  chromeAvailable: () => call('chrome_available'), openTaskManager: () => call('open_task_manager'),
  configLoad: () => call('config_load'), configSave: (cfg) => call('config_save', { cfg }),
  deepyStatus: () => call('deepy_status'), deepyActivate: (e) => call('deepy_activate', { engine: e }),
  deepySet: (mode, engine, enhancer) => call('deepy_set', { mode, engine, enhancer }),
  llmEnginesList: () => call('llm_engines_list'), llmEngineInstall: (e) => call('llm_engine_install', { engine: e }),
  llmEngineServe: (e, a) => call('llm_engine_serve', { engine: e, action: a }), llmEngineAuth: (e) => call('llm_engine_auth', { engine: e }),
  memoryProfileRead: () => call('memory_profile_read'), memoryProfileApply: (s) => call('memory_profile_apply', { settings: s }),
  notifierConfig: () => call('notifier_config'), notifierSet: (c) => call('notifier_set', { cfg: c }),
  notifierTest: (c) => call('notifier_test', { cfg: c }), notifierEnsure: () => call('notifier_ensure'),
  pulsebarHide: () => call('pulsebar_hide'), setAutoStart: (e) => call('set_auto_start', { enabled: e }),
  setThemeFollowSystem: (e) => call('set_theme_follow_system', { enabled: e }),
  setNotificationsEnabled: (e) => call('set_notifications_enabled', { enabled: e }),
  checkUpdate: (o) => call('check_update', { opts: o }), downloadUpdate: (o) => call('download_update', { opts: o }), installUpdate: () => call('install_update'),
  getWangpLocalVersion: () => call('get_wangp_local_version'), getWangpUpstreamInfo: () => call('get_wangp_upstream_info'),
  getDesktopGitInfo: () => call('get_desktop_git_info'), getDesktopVersion: () => call('get_desktop_version'),
  getWangpVersion: () => call('get_wangp_version'), reportIssue: () => call('report_issue'),
  createDesktopShortcut: () => call('create_desktop_shortcut'),
  checkPackageUpdates: (v) => call('check_package_updates', { versions: v }),
  upgradePackage: (p) => call('upgrade_package', { pkg: p }), installPackage: (p) => call('install_package', { pkg: p }),
  uninstallPackage: (p) => call('uninstall_package', { pkg: p }), checkPackage: (p) => call('check_package', { pkg: p }),
  restoreRequirements: () => call('restore_requirements'),
  // events: no-op shims (Tauri uses listen(), not on())
  onSetupOutput: () => () => {}, onLaunchLog: () => () => {}, onWangpExit: () => () => {},
  onUpdateStatus: () => () => {}, onBvNavState: () => () => {},
};

// expose for reference/renderer drop-in: `window.w2gp = w2gp` makes 3332-line app.js run as-is
if (typeof window !== 'undefined') window.w2gp = w2gp;
