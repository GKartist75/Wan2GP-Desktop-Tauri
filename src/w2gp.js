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
  // Native-embed bookkeeping: true while a real child Webview (not an iframe)
  // owns the Gradio view. A native child composites ABOVE the DOM, so overlay
  // tricks (floating terminal over the view) don't apply — callers must hide
  // or shrink it via bvSyncBounds instead.
  let nativeEmbed = false;
  window.__syncNativeBounds = function(logIt) {
    try {
      if (!nativeEmbed) return;
      const host = document.getElementById('webviewContainer');
      if (!host || host.classList.contains('hidden')) return;
      const r = host.getBoundingClientRect();
      if (!r.width || !r.height) return;
      // The child is an OS window above ALL DOM (incl. the topbar with its
      // metrics) — clamp so it can never cover the topbar, whatever the
      // layout does (banners, docks, DPI scaling).
      let x = r.left, y = r.top, w = r.width, h = r.height;
      try {
        const tb = document.querySelector('.topbar');
        if (tb) { const minY = tb.getBoundingClientRect().bottom; if (y < minY) { h -= (minY - y); y = minY; } }
      } catch {}
      x = Math.max(0, x); y = Math.max(0, y);
      w = Math.min(w, window.innerWidth - x); h = Math.min(h, window.innerHeight - y);
      if (w < 10 || h < 10) return;
      if (logIt) console.log('[embed] native bounds', { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) });
      call('bv_sync_bounds', { x, y, w, h }).catch(() => {});
    } catch {}
  };
  if (!window.__nativeBoundsWired) { window.addEventListener('resize', () => { try { window.__syncNativeBounds(); } catch {} }); window.__nativeBoundsWired = true; }
  window.__fitBrowserView = function() {
    try {
      const host = document.getElementById('webviewContainer');
      const c = document.getElementById('tauri-browser-view');
      if (!host || !c || host.classList.contains('hidden')) return;
      const r = host.getBoundingClientRect();
      if (!r.width || !r.height) return;
      const h = Math.max(300, Math.floor(window.innerHeight - r.top - 8));
      host.style.height = h + 'px';
      c.style.height = h + 'px';
      const f = c.querySelector('iframe');
      if (f) f.style.height = h + 'px';
    } catch {}
  };
  if (!window.__bvFitWired) { window.addEventListener('resize', () => { try { window.__fitBrowserView(); } catch {} }); window.__bvFitWired = true; }
  const w2gp = {
    platform: navigator.platform.includes('Win') ? 'win32' : navigator.platform.includes('Mac') ? 'darwin' : 'linux',
    checkInstalled: () => call('check_installed'), detectGpu: () => call('detect_gpu'), detectGpus: () => call('detect_gpus'),
    detectHardware: () => call('detect_hardware'), getHardwareProfile: () => call('get_hardware_profile'), getSystemMetrics: () => call('get_system_metrics'),
    autoTuneDetect: () => call('auto_tune_detect'), autoTuneRecommend: (hw, opts) => call('auto_tune_recommend', { hw, opts }),
    install: (envType) => call('install', { envType }), reinstall: (opts) => call('reinstall', { options: opts ?? null }), uninstall: (opts) => call('uninstall', { options: opts ?? null }),
    update: () => call('update'), syncKernels: () => call('sync_kernels'), installPlan: () => call('install_plan'), validateInstall: () => call('validate_install'),
    classifyTarget: () => call('classify_target'), pythonPreflight: () => call('python_preflight'),
    restoreBackup: () => call('restore_backup'),
    manageList: () => call('manage_list'), manageSetActive: (name) => call('manage_set_active', { name }), uninstallEnv: (name) => call('uninstall_env', { name }),
    uvCacheInfo: () => call('uv_cache_info'), uvCacheSize: () => call('uv_cache_size'), uvCacheClean: (a) => call('uv_cache_clean', { action: a }),
    checkCommand: (cmd) => call('check_command', { cmd }), installPrerequisite: (tool) => call('install_prerequisite', { tool }),
    getInstallPaths: () => call('get_install_paths'), getDiskSpace: (path) => call('get_disk_space', { path }),
    openFolder: (p) => call('open_folder', { path: p }), setDataDir: (dir) => call('set_data_dir', { dir }),
    resetDataDir: () => call('reset_data_dir'), migrateToPreferred: (c) => call('migrate_to_preferred', { choices: c }),
    moveFolder: (src, dst) => call('move_folder', { src, dst }), migrateChoose: () => call('migrate_choose'),
    folderSize: (path) => call('folder_size', { path }),
    downloadsSince: (sinceMs) => call('downloads_since', { sinceMs }).then(r => (r && r.files) || []).catch(() => []),
    saveDownloadedFile: (name, dir) => call('save_downloaded_file', { name, dir: dir ?? null }),
    // One-shot WebView2/RAM footprint ({webviewMb, webviewProcs, launcherMb, top[]}).
    webviewMemory: () => call('webview_memory'),
    bvSyncBounds: () => { try { window.__syncNativeBounds(); } catch {} return Promise.resolve({ ok: true }); },
    bvSyncBoundsRect: (r) => call('bv_sync_bounds', r).catch(() => ({ ok: false })),
    // Browser-with-ask finish: staged file → native Save-As dialog → move.
    saveStagedDownload: (path, dir) => call('save_staged_download', { path, dir: dir ?? null }),
    isNativeEmbed: () => nativeEmbed,
    onDownloadFinished: (cb) => { listen('download-finished', cb); return () => {}; },
    onConsoleMirror: (cb) => { listen('console-mirror', cb); return () => {}; },
    // Fire-and-forget (hot path — must never throw into appendLog).
    mirrorConsole: (text) => { try { call('mirror_console', { text }).catch(() => {}); } catch {} return Promise.resolve({ ok: true }); },
    onDownloadStarted: (cb) => { listen('download-started', cb); return () => {}; },
    onGradioPageLoad: (cb) => { listen('gradio-page-load', cb); return () => {}; },
    isDataDirRoaming: () => call('is_data_dir_roaming'),
    writeWgpConfig: (cfg) => call('write_wgp_config', { cfg }), selectFolder: () => call('select_folder'),
    resetWgpConfig: () => call('reset_wgp_config'),
    confirmDialog: async (opts) => {
        try { const r = await call('confirm_dialog', { opts }); if (typeof r==='string') return r; if (r && typeof r.choice==='string') return r.choice; if (r && typeof r.response==='number') return r.response===0 ? 'ok' : 'cancel'; if (r && r.ok) return 'ok'; return r; } catch { return 'cancel'; }
    }, detectModelFolders: () => call('detect_model_folders'),
    getModelPaths: () => call('get_model_paths'), repairSettings: () => call('repair_settings'),
    getStatus: () => call('get_status'), launch: (mode) => call('launch', { mode }), launchWebview: () => call('launch_webview'),
    stopWangp: () => call('stop_wangp'), stopAllServers: () => call('stop_all_servers'), popoutWebview: async (url) => { const u = url || 'http://localhost:7861'; try { await call('open_external', { url: u }); } catch {} window.open(u, '_blank'); return {ok:true}; },
    // ponytail: BrowserView → embedded iframe in webviewContainer (Tauri) — simple, no separate window.
    // Manage → Launch → "Desktop embed" switch: embedMode=native renders Gradio in a
    // real child Webview (backend owns it, downloads arrive as download-finished
    // events); anything else keeps the iframe path. Backend returns the active mode.
    createBrowserView: async (url, opts) => {
        const u = url || 'http://localhost:7861';
        // close any previous WebviewWindow if it exists (from previous separate-window attempt)
        try { const { WebviewWindow } = window.__TAURI__.webviewWindow; const win = await WebviewWindow.getByLabel('wan2gp-view'); if (win) await win.close(); } catch {}
        document.getElementById('tauri-browser-view')?.remove();
        let created = { ok: true, mode: 'iframe' };
        // Pass the wanted mode explicitly (backend falls back to its config read).
        // If native was wanted but the backend reports iframe, it failed — flag
        // it so app.js can warn instead of silently running the old renderer.
        let want = 'native';
        try { const _cfg = await w2gp.configLoad().catch(() => ({})); want = (_cfg && _cfg.embedMode === 'iframe') ? 'iframe' : 'native'; } catch {}
        try { created = await call('create_browser_view', { url: u, opts: Object.assign({}, opts ?? null, { mode: want }) }); } catch (e) { created = { ok: false, mode: 'iframe', error: String((e && e.message) || e) }; }
        if (want === 'native' && (!created || created.mode !== 'native')) created.fallback = true;
        nativeEmbed = !!(created && created.mode === 'native');
        if (nativeEmbed) {
            // Backend owns the view — just make the host measurable and sync bounds.
            const host = document.getElementById('webviewContainer') || document.body;
            if (host.id === 'webviewContainer') { host.classList.remove('hidden'); host.style.display = 'flex'; host.style.flex = '1'; host.style.minHeight = '0'; host.style.position = 'relative'; }
            try { const db = document.getElementById('dashBody'); if (db) db.style.display = 'none'; } catch {}
            console.log('[tauri] native child webview created for', u);
            setTimeout(() => { try { window.__syncNativeBounds(); } catch {} }, 50);
            setTimeout(() => { try { window.__syncNativeBounds(); } catch {} }, 800);
            return { ok: true, mode: 'native' };
        }
        const host = document.getElementById('webviewContainer') || document.body;
        const isWebviewHost = host.id === 'webviewContainer';
        // ensure host is visible and has height — webviewContainer is flex:1 inside dashboard (flex column)
        if (isWebviewHost) { host.classList.remove('hidden'); host.style.display = 'flex'; host.style.flex = '1'; host.style.minHeight = '0'; host.style.height = 'calc(100vh - 44px)'; host.style.position = 'relative'; }
        const c = document.createElement('div');
        c.id = 'tauri-browser-view';
        c.style.cssText = 'flex:1;display:flex;flex-direction:column;background:#111;min-height:0;width:100%;height:100%;overflow:auto;';
        // Permissions-Policy: the embed must behave like the same page in a real
        // browser tab. Anything missing here is silently denied vs. Explorer:
        // fullscreen (expand), autoplay (gallery video/audio previews),
        // camera/microphone (Gradio audio/image capture inputs), clipboard
        // (copy results), downloads (gallery save — blocked without the token),
        // picture-in-picture, display-capture, web-share. No `sandbox`
        // attribute on purpose (it would cripple scripts/uploads).
        c.innerHTML = `<iframe src="${u}" style="flex:1;width:100%;height:100%;border:0;background:#111;display:block;" allow="fullscreen; autoplay; camera; microphone; clipboard-read; clipboard-write; allow-downloads; allow-downloads-without-user-activation; picture-in-picture; display-capture; web-share"></iframe>`;
        host.appendChild(c);
        // Carry over the zoom slider's current value so a rebuilt view doesn't
        // silently reset to 100% while the label claims otherwise.
        try {
          const pct = parseInt((document.getElementById('zoomSlider') || {}).value) || 100;
          const f0 = c.querySelector('iframe');
          if (f0 && pct !== 100) f0.style.zoom = (pct / 100);
        } catch {}
        // Pixel-exact fit (banner-aware): percentage heights can collapse to the
        // 150px iframe default on some Chromium/GPU stacks (same class as the
        // #39/#45 blank-screen issue) — measure the live position and pin real
        // pixel heights down the whole chain so "half screen" is impossible.
        try { window.__fitBrowserView(); } catch {}
        setTimeout(() => { try { window.__fitBrowserView(); } catch {} }, 800);
        // hide dashBody, show webviewContainer — app.js also does this, but enforce
        try { const db=document.getElementById('dashBody'); if(db) db.style.display='none'; } catch {}
        // ensure dashBody stays hidden while iframe shows (app.js does this, but enforce)
        try { const db=document.getElementById('dashBody'); if(db) db.style.display='none'; } catch {}
        console.log('[tauri] BrowserView iframe created for', u, '— if blank, check F12 Network for', u, 'and X-Frame-Options');
        return { ok: true, mode: 'iframe' };
    },
    hideBrowserView: async (reason) => {
        // Native child composites above the DOM — no "keep visible under the
        // terminal" trick: any hide (incl. 'term') hides the child. The docked
        // terminal path re-shows + shrinks it via bvSyncBounds instead.
        if (nativeEmbed) { try { await call('hide_browser_view'); } catch {} return { ok: true }; }
        // ponytail: docked terminal calls hideBrowserView('term') to shrink, not hide — keep iframe visible
        if (reason === 'term') { console.log('[tauri] hideBrowserView(term) — keep iframe visible'); try{await call('hide_browser_view');}catch{} return {ok:true}; }
        try { const { WebviewWindow } = window.__TAURI__.webviewWindow; const win = await WebviewWindow.getByLabel('wan2gp-view'); if (win) { try{ await win.hide(); }catch{} } } catch {}
        const c=document.getElementById('tauri-browser-view'); if(c) c.style.display='none'; try{await call('hide_browser_view');}catch{} return {ok:true};
    },
    destroyBrowserView: async () => {
        nativeEmbed = false;
        try { const { WebviewWindow } = window.__TAURI__.webviewWindow; const win = await WebviewWindow.getByLabel('wan2gp-view'); if (win) { try{ await win.close(); }catch{} } } catch {}
        document.getElementById('tauri-browser-view')?.remove();
        try { const wc=document.getElementById('webviewContainer'); if(wc){ wc.classList.add('hidden'); wc.innerHTML=''; } const db=document.getElementById('dashBody'); if(db) db.style.display='flex'; } catch {}
        try{await call('destroy_browser_view');}catch{} return {ok:true};
    },
    detachBrowserView: async () => { const c=document.getElementById('tauri-browser-view'); if(c) c.style.display='none'; try{await call('detach_browser_view');}catch{} return {ok:true}; },
    reattachBrowserView: async () => { const c=document.getElementById('tauri-browser-view'); if(c) c.style.display='flex'; try{await call('reattach_browser_view');}catch{} return {ok:true}; },
    createTermView: () => call('create_term_view'), destroyTermView: () => call('destroy_term_view'),
    toggleTermWindow: () => call('toggle_term_window'),
    // Term-window buttons (dock/close/export live in the separate window and
    // have no access to the main window's functions — routed via backend).
    setDock: (d) => call('term_set_dock', { dock: d }),
    closeTerm: () => call('destroy_term_view'),
    exportLogs: (text) => call('export_logs', { text: text ?? null }),
    onTermSetDock: (cb) => { listen('term-set-dock', cb); return () => {}; },
    bvNavigate: (a) => call('bv_navigate', { action: a }), bvSetZoom: (f) => call('bv_set_zoom', { factor: f }), bvSetDock: (d) => { console.log('[tauri] bvSetDock', d, '— keep iframe visible'); try{ return call('bv_set_dock', { dock: d }); }catch{ return Promise.resolve({ok:true}); } },
    getLogHistory: () => call('get_log_history'),
    openExternal: async (url) => { const u = url || 'http://localhost:7861'; try { await window.__TAURI__.core.invoke('plugin:opener|open_url', { url: u }); } catch { try { window.open(u, '_blank'); } catch {} } try { await call('open_external', { url: u }); } catch {} return {ok:true}; },
    detectBrowsers: () => call('detect_browsers'),
    launchBrowser: async (url) => { const u = url || 'http://localhost:7861'; try { const r = await call('launch_browser', { url: u }); if (r && (r.success || r.ok)) return r; } catch {} try { await window.__TAURI__.core.invoke('plugin:opener|open_url', { url: u }); } catch { window.open(u, '_blank'); } return {ok:true}; },
    launchBrowserNoGpu: async (url) => { const u = url || 'http://localhost:7861'; try { const r = await call('launch_browser_no_gpu', { url: u }); if (r && (r.success || r.ok)) return r; } catch {} try { await window.__TAURI__.core.invoke('plugin:opener|open_url', { url: u }); } catch { window.open(u, '_blank'); } return {ok:true}; },
    chromeAvailable: () => call('chrome_available'), openTaskManager: () => call('open_task_manager'),
    configLoad: () => call('config_load'), configSave: (cfg) => call('config_save', { cfg }),
    deepyStatus: () => call('deepy_status'), deepyActivate: (e) => call('deepy_activate', { engine: e }),
    deepySet: (mode, engine, enhancer) => call('deepy_set', { mode, engine, enhancer }),
    llmEnginesList: () => call('llm_engines_list'), llmEngineInstall: (e) => call('llm_engine_install', { engine: e }), llmEngineUninstall: (e) => call('llm_engine_uninstall', { engine: e }),
    llmEngineServe: (e, a) => call('llm_engine_serve', { engine: e, action: a }), llmEngineAuth: (e) => call('llm_engine_auth', { engine: e }),
    memoryProfileRead: () => call('memory_profile_read'), memoryProfileApply: (s) => call('memory_profile_apply', { settings: s }),
    tsFailsafeApply: () => call('troubleshoot_failsafe_apply'), tsCudaCheck: () => call('troubleshoot_cuda_check'),
    tsPortStatus: () => call('troubleshoot_port_status'), tsPortFix: (a) => call('troubleshoot_port_fix', { action: a }),
    tsDebugBundle: () => call('troubleshoot_debug_bundle'), tsTritonTest: () => call('troubleshoot_triton_test'),
    tsGpuCompute: () => call('troubleshoot_gpu_compute'),
    tsTritonClear: (fb) => call('troubleshoot_triton_clear', { fallbackSdpa: !!fb }),
    notifierConfig: () => call('notifier_config'), notifierSet: (c) => call('notifier_set', { cfg: c }),
    notifierTest: (c) => call('notifier_test', { cfg: c }), notifierEnsure: () => call('notifier_ensure'),
    dlss5Status: () => call('dlss5_status'), installDlss5: (force) => call('install_dlss5', { force: !!force }),
    pluginsList: () => call('plugins_list'), pluginInstall: (url) => call('plugin_install', { url }),
    pluginCheckUpdate: (id) => call('plugin_check_update', { id }), pluginCheckUpdates: () => call('plugin_check_updates'), pluginUpdate: (id) => call('plugin_update', { id }),
    pluginUninstall: (id) => call('plugin_uninstall', { id }),
    pluginRefreshCatalog: () => call('plugin_refresh_catalog'),
    setAutoStart: (e) => call('set_auto_start', { enabled: e }),
    setThemeFollowSystem: (e) => call('set_theme_follow_system', { enabled: e }),
    setNotificationsEnabled: (e) => call('set_notifications_enabled', { enabled: e }),
    checkUpdate: (o) => call('check_update', { opts: o }), downloadUpdate: (o) => call('download_update', { opts: o }), installUpdate: () => call('install_update'),
    getWangpLocalVersion: () => call('get_wangp_local_version'), getWangpUpstreamInfo: () => call('get_wangp_upstream_info'),
    getDesktopGitInfo: () => call('get_desktop_git_info'), getDesktopVersion: () => call('get_desktop_version'),
    detectElectron: () => call('detect_electron'), uninstallElectron: () => call('uninstall_electron'),
    getWangpVersion: () => call('get_wangp_version'), reportIssue: () => call('report_issue'),
    createDesktopShortcut: () => call('create_desktop_shortcut'),
    checkPackageUpdates: (v) => call('check_package_updates', { versions: v }),
    upgradePackage: (p) => call('upgrade_package', { pkg: p }), installPackage: (p) => call('install_package', { pkg: p }),
    uninstallPackage: (p) => call('uninstall_package', { pkg: p }), checkPackage: (p) => call('check_package', { pkg: p }),
    restoreRequirements: () => call('restore_requirements'),
    getCrashRecoveryInfo: () => call('get_crash_recovery_info'), uiModeSet: (m) => call('ui_mode_set', { mode: m }),
    // event bridges: Electron ipcRenderer.on → Tauri listen (returns unlisten fn) — with debug log for phases
    onSetupOutput: (cb) => { listen('setup-output', cb); return ()=>{}; },
    onDlss5Progress: (cb) => { listen('dlss5-progress', cb); return ()=>{}; },
    onInstallProgress: (cb) => { listen('install-progress', cb); return ()=>{}; },
    onSetupPhase: (cb) => { console.log('[w2gp] onSetupPhase registered'); listen('setup-phase', (p)=>{ console.log('[w2gp] setup-phase', p); try{cb(p);}catch(e){console.error(e);} }); return ()=>{}; },
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
  // ponytail: dummy plugins for Gradio iframe that does window.parent.w2gp.plugins (MotionDesigner bridge) — prevents "Cannot read properties of undefined (reading 'plugins')" and black screen
  window.w2gp.plugins = {};
  // also expose for module users
  window.w2gpReady = true;
})();
