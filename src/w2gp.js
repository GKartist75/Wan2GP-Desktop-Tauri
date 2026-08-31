// ponytail: plain script — no import/export — loads before app.js and sets window.w2gp via Tauri invoke
(function(){
  const inv = (name, args) => window.__TAURI__.core.invoke(name, args);
  function call(name, args){
    const key = name.replace(/[-:]/g, '_');
    return inv(key, args);
  }
  // Tauri event shims: Electron used ipcRenderer.on; Tauri uses listen()
  function listen(event, cb){
    try { return window.__TAURI__.event.listen(event, e => cb(e.payload)); } catch(e){ return Promise.resolve(()=>{}); }
  }
  const w2gp = {
    platform: navigator.platform.includes('Win') ? 'win32' : navigator.platform.includes('Mac') ? 'darwin' : 'linux',
    checkInstalled: () => call('check_installed'), detectGpu: () => call('detect_gpu'), detectGpus: () => call('detect_gpus'),
    detectHardware: () => call('detect_hardware'), getHardwareProfile: () => call('get_hardware_profile'), getSystemMetrics: () => call('get_system_metrics'),
    autoTuneDetect: () => call('auto_tune_detect'), autoTuneRecommend: (hw, opts) => call('auto_tune_recommend', { hw, opts }),
    install: (envType) => call('install', { envType }), reinstall: () => call('reinstall'), uninstall: () => call('uninstall'),
    update: () => call('update'), syncKernels: () => call('sync_kernels'), installPlan: () => call('install_plan'), validateInstall: () => call('validate_install'),
    manageList: () => call('manage_list'), manageSetActive: (name) => call('manage_set_active', { name }), uninstallEnv: (name) => call('uninstall_env', { name }),
    uvCacheInfo: () => call('uv_cache_info'), uvCacheSize: () => call('uv_cache_size'), uvCacheClean: (a) => call('uv_cache_clean', { action: a }),
    checkCommand: (cmd) => call('check_command', { cmd }), installPrerequisite: (tool) => call('install_prerequisite', { tool }),
    getInstallPaths: () => call('get_install_paths'), getDiskSpace: (path) => call('get_disk_space', { path }),
    openFolder: (p) => call('open_folder', { path: p }), setDataDir: (dir) => call('set_data_dir', { dir }),
    resetDataDir: () => call('reset_data_dir'), migrateToPreferred: (c) => call('migrate_to_preferred', { choices: c }),
    moveFolder: (src, dst) => call('move_folder', { src, dst }), migrateChoose: () => call('migrate_choose'),
    isDataDirRoaming: () => call('is_data_dir_roaming'),
    writeWgpConfig: (cfg) => call('write_wgp_config', { cfg }), selectFolder: () => call('select_folder'),
    confirmDialog: (opts) => call('confirm_dialog', { opts }), detectModelFolders: () => call('detect_model_folders'),
    getModelPaths: () => call('get_model_paths'), repairSettings: () => call('repair_settings'),
    getStatus: () => call('get_status'), launch: (mode) => call('launch', { mode }), launchWebview: () => call('launch_webview'),
    stopWangp: () => call('stop_wangp'), popoutWebview: async (url) => { const u = url || 'http://localhost:7860'; try { await call('open_external', { url: u }); } catch {} window.open(u, '_blank'); return {ok:true}; },
    // ponytail: BrowserView → fixed iframe overlay (high z-index, flex) — fixes black screen / CSS hide
    createBrowserView: async (url, opts) => {
        const u = url || 'http://localhost:7860';
        // remove any stale container
        document.getElementById('tauri-browser-view')?.remove();
        const c = document.createElement('div');
        c.id = 'tauri-browser-view';
        // fixed overlay above #app (which is absolute inset:0) — use 9999 z-index and inset to avoid being covered
        c.style.cssText = 'position:fixed;inset:60px 0 120px 0;background:#0f0f0f;z-index:9999;display:flex;flex-direction:column;border-top:1px solid #333;';
        c.innerHTML = `<div style="display:flex;align-items:center;gap:8px;padding:6px 12px;background:#1a1a1a;border-bottom:1px solid #2a2a2a;font-size:12px;color:#aaa;flex-shrink:0;"><span style="color:#eee">● Wan2GP</span><span style="opacity:0.6;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${u}</span><button id="tauri-bv-close" style="margin-left:auto;background:#2a2a2a;color:#eee;border:1px solid #444;padding:4px 12px;border-radius:6px;cursor:pointer;">✕ Close</button><button id="tauri-bv-popout" style="background:#2a2a2a;color:#eee;border:1px solid #444;padding:4px 12px;border-radius:6px;cursor:pointer;">Open in Browser</button></div><iframe src="${u}" style="flex:1;width:100%;height:100%;border:0;background:#111;display:block;" allow="fullscreen; clipboard-read; clipboard-write" sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-downloads"></iframe>`;
        document.body.appendChild(c);
        // ensure dashBody stays hidden while iframe shows (app.js does this, but enforce)
        try { const db=document.getElementById('dashBody'); if(db) db.style.display='none'; } catch {}
        document.getElementById('tauri-bv-close')?.addEventListener('click', () => { c.remove(); try { const db=document.getElementById('dashBody'); if(db) db.style.display='flex'; } catch {} });
        document.getElementById('tauri-bv-popout')?.addEventListener('click', async () => { try { await window.__TAURI__.core.invoke('plugin:opener|open_url', { url: u }); } catch { window.open(u,'_blank'); } });
        console.log('[tauri] BrowserView iframe created for', u, '— if blank, check F12 Network for', u, 'and X-Frame-Options');
        try { await call('create_browser_view', { url: u, opts }); } catch {}
        return {ok:true};
    },
    hideBrowserView: async () => { const c=document.getElementById('tauri-browser-view'); if(c) c.style.display='none'; try{await call('hide_browser_view');}catch{} return {ok:true}; },
    destroyBrowserView: async () => { document.getElementById('tauri-browser-view')?.remove(); try{await call('destroy_browser_view');}catch{} return {ok:true}; },
    detachBrowserView: async () => { const c=document.getElementById('tauri-browser-view'); if(c) c.style.display='none'; try{await call('detach_browser_view');}catch{} return {ok:true}; },
    reattachBrowserView: async () => { const c=document.getElementById('tauri-browser-view'); if(c) c.style.display='flex'; try{await call('reattach_browser_view');}catch{} return {ok:true}; },
    createTermView: () => call('create_term_view'), destroyTermView: () => call('destroy_term_view'),
    bvNavigate: (a) => call('bv_navigate', { action: a }), bvSetZoom: (f) => call('bv_set_zoom', { factor: f }), bvSetDock: (d) => call('bv_set_dock', { dock: d }),
    getLogHistory: () => call('get_log_history'),
    openExternal: async (url) => { const u = url || 'http://localhost:7860'; try { await window.__TAURI__.core.invoke('plugin:opener|open_url', { url: u }); } catch { try { window.open(u, '_blank'); } catch {} } try { await call('open_external', { url: u }); } catch {} return {ok:true}; },
    detectBrowsers: () => call('detect_browsers'),
    launchBrowser: async (url) => { const u = url || 'http://localhost:7860'; try { await window.__TAURI__.core.invoke('plugin:opener|open_url', { url: u }); } catch { window.open(u, '_blank'); } try { await call('launch_browser', { url: u }); } catch {} return {ok:true}; },
    launchBrowserNoGpu: async (url) => { const u = url || 'http://localhost:7860'; try { await window.__TAURI__.core.invoke('plugin:opener|open_url', { url: u }); } catch { window.open(u, '_blank'); } try { await call('launch_browser_no_gpu', { url: u }); } catch {} return {ok:true}; },
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
    getCrashRecoveryInfo: () => call('get_crash_recovery_info'), uiModeSet: (m) => call('ui_mode_set', { mode: m }),
    // event bridges: Electron ipcRenderer.on → Tauri listen (returns unlisten fn)
    onSetupOutput: (cb) => { listen('setup-output', cb); return ()=>{}; },
    onSetupPhase: (cb) => { listen('setup-phase', cb); return ()=>{}; },
    onSetupProfile: (cb) => { listen('setup-profile', cb); return ()=>{}; },
    onLaunchLog: (cb) => { listen('launch-log', cb); return ()=>{}; },
    onWangpExit: (cb) => { listen('wangp-exit', cb); return ()=>{}; },
    onUpdateStatus: (cb) => { listen('update-status', cb); return ()=>{}; },
    onBvNavState: (cb) => { listen('bv-nav-state', cb); return ()=>{}; },
    onTermDockChanged: (cb) => { listen('term-dock-changed', cb); return ()=>{}; },
    onTermClosed: (cb) => { listen('term-closed', cb); return ()=>{}; },
    onOpenMigration: (cb) => { listen('open-migration', cb); return ()=>{}; },
    onMigrationProgress: (cb) => { listen('migration-progress', cb); return ()=>{}; },
    onSystemThemeChange: (cb) => { listen('system-theme-changed', cb); return ()=>{}; },
    onBvCrashRecovered: (cb) => { listen('bv-crash-recovered', cb); return ()=>{}; }
  };
  window.w2gp = w2gp;
  // also expose for module users
  window.w2gpReady = true;
})();
