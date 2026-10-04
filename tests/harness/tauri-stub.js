/* tauri-stub.js — test harness only. Loaded BEFORE app scripts by harness/serve.js.
   Answers every IPC call the renderer makes with a canned shape so the UI can
   be driven in a plain browser. NOT shipped: harness/ lives under tests/. */
;(function () {
  'use strict'
  var STATUS = {
    ok: true, installed: true, running: true, mode: 'zero',
    hasActiveEnv: true, envPath: 'C:\\Wan2GP\\envs\\py311',
    deepyEnabled: true, deepyType: 'zero', enhancerEnabled: 4,
    enhancerMode: 1, currentEngine: 'qwen35_9b',
    promptEnhancer: true, promptEnhancerQuantization: 'gguf',
    sessionMode: 'selectable', sessionResetMode: 'reset_session',
    sessionGalleryMediaMode: 'copy', engines: [], profiles: {},
    kernelProfile: 'RTX_50',
    gpu: { vendor: 'NVIDIA', name: 'NVIDIA GeForce RTX 5090', vramGb: 32 },
    versions: { 'claude-agent-sdk': '0.1.66', torch: '2.10.0', wan2gp: '1.3.5' },
    wheels: [
      { key: 'nunchaku_cu13', label: 'Nunchaku', pipName: 'nunchaku', configured: '1.2.1', installed: true, version: '1.2.1' },
      { key: 'gguf', label: 'GGUF (llamacpp)', pipName: 'llamacpp_gguf_cuda', configured: '1.0.25', installed: false, version: null },
    ],
    metrics: { ramTotalGb: 64, ramUsedGb: 22, vramTotalGb: 32, vramUsedGb: 4 },
    ports: { serverPort: 7860, deepyPort: 7861, logPort: 7862 },
    desktop: { version: '0.9.2' },
  }
  var ENGINES = {
    ok: true, hasActiveEnv: true,
    engines: [
      { id: 'claude-code', label: 'Claude Code', desc: 'Anthropic Claude Code CLI + Python bridge',
        cli: 'claude', cliOnPath: true, pipPackage: 'claude_agent_sdk', pipInstalled: true,
        install: { mode: 'pip', spec: 'claude-agent-sdk==0.1.66' }, external: false,
        auth: { docsUrl: 'https://code.claude.com/docs/en/authentication' },
        notes: 'Pinned to 0.1.66 on purpose.' },
      { id: 'codex', label: 'OpenAI Codex', desc: 'OpenAI Codex CLI (npm)',
        cli: 'codex', cliOnPath: false, pipPackage: null, pipInstalled: null,
        install: { mode: 'npm', spec: '@openai/codex', global: true }, external: true,
        authHint: 'Sign in via a Deepy request in Wan2GP.' },
      { id: 'opencode', label: 'OpenCode', desc: 'Universal-provider agent',
        cli: 'opencode', cliOnPath: true, pipPackage: null, pipInstalled: null,
        install: { mode: 'npm', spec: 'opencode-ai', global: true }, external: true,
        serve: { cmd: 'opencode', args: ['serve', '--hostname', '127.0.0.1', '--port', '4096'] },
        serverUrl: 'http://127.0.0.1:4096', serverRunning: false,
        authHint: 'In the OpenCode UI run /connect.' },
    ],
  }
  var CONFIG = {
    ok: true,
    serverPort: 7860, deepyPort: 7861, logPort: 7862,
    modelFolder: 'C:\\Wan2GP\\models', outputFolder: 'C:\\Wan2GP\\outputs',
    theme: 'dark', accent: 'mono', launcherGpu: 'auto',
  }
  var calls = []
  function payload(name, args) {
    // The boot path only reaches the dashboard (and therefore every status poll,
    // refreshDashboard, refreshLLMEngines) when the install probe reports BOTH a
    // repo and an env — see app.js:2149. Without this the harness looks like a
    // broken app: the LLM cards never render because nothing ever asks.
    if (name === 'check_installed') return { ok: true, repo: true, env: true }
    if (name === 'get_status' || name === 'status') return STATUS
    // memory_profile_apply returns success/applied, NOT just ok — the callers
    // branch on r.success, so a stub without it makes every save look failed.
    if (name === 'memory_profile_apply')
      return { ok: true, success: true, applied: Object.keys(args && args.settings || {}), snapshot: 'harness' }
    if (name === 'llm_engines_list') return ENGINES
    if (name === 'config_load') return CONFIG
    if (name === 'get_desktop_version') return '0.9.2'
    if (name === 'detect_gpu' || name === 'get_gpu_info') return STATUS.gpu
    if (name === 'auto_tune_recommend') return { ok: true, tiers: { ramTier: 'high', vramTier: 'high' }, notes: [] }
    if (name === 'deepy_status') return { ok: true, available: true, mode: 'prime', deepyEnabled: true, deepyType: 'prime', currentEngine: 'opencode', promptEnhancer: true, enhancerEnabled: 4, promptEnhancerQuantization: 'gguf', sessionMode: 'selectable', sessionResetMode: 'reset_session', sessionGalleryMediaMode: 'copy', enhancerMode: 1, engines: [], profiles: {} }
    if (name === 'plugins_list') return { ok: true, plugins: [] }
    if (name === 'config_backups_list') return { ok: true, backups: [] }
    if (name === 'upstream_changelog') return { ok: true, commits: [] }
    if (name === 'library_models' || name === 'library_loras' || name === 'library_finetunes') return { ok: true, items: [] }
    if (name === 'workspace_list') return { ok: true, workspaces: [] }
    if (name === 'triton_status' || name === 'long_paths_status') return { ok: true, enabled: true }
    if (name === 'dlss5_classify') return { ok: true, tier: 'supported', found: true }
    if (name === 'log_server_status') return { ok: true, running: false, port: 7862, lan: false, samePc: null, phone: null, phoneUnavailable: true }
    if (name === 'uv_cache_info') return { ok: true, sizeGb: 0 }
    if (name === 'deepy_web_status') return { ok: true, running: false, mode: 'disabled' }
    return { ok: true }
  }
  window.__TAURI__ = {
    core: {
      invoke: function (name, args) {
        calls.push(name)
        window.__harnessCalls = calls
        try { return Promise.resolve(payload(name, args)) } catch (e) { return Promise.reject(e) }
      },
    },
    event: { listen: function () { return Promise.resolve(function () {}) } },
    webviewWindow: { WebviewWindow: { getByLabel: function () { return Promise.resolve(null) } } },
  }
  window.__harnessReady = true
})();
