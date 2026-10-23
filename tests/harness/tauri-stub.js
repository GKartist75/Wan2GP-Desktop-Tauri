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
  // Config can be seeded from the URL (?theme=light&themeFollowSystem=1) so a
  // persisted preference survives a reload and can be exercised at all.
  // Without it CONFIG is hardcoded dark, which hides every theme-persistence bug.
  try {
    var q = new URLSearchParams(location.search)
    if (q.has('theme')) CONFIG.theme = q.get('theme')
    if (q.has('themeFollowSystem')) CONFIG.themeFollowSystem = q.get('themeFollowSystem') === '1'
  } catch (e) {}
  var calls = []
  function payload(name, args) {
    // The boot path only reaches the dashboard (and therefore every status poll,
    // refreshDashboard, refreshLLMEngines) when the install probe reports BOTH a
    // repo and an env — see app.js:2149. Without this the harness looks like a
    // broken app: the LLM cards never render because nothing ever asks.
    if (name === 'check_installed') return { ok: true, repo: true, env: true }
    // Prerequisite gate + panel: mixed state so Install vs Reinstall labels,
    // versions and the missing copy all render in the harness.
    if (name === 'check_command')
      return { ok: true, cmd: args && args.cmd, found: true }
    if (name === 'prerequisite_report')
      return {
        tools: [
          { tool: 'git', ready: true, path: 'C:\\Program Files\\Git\\cmd\\git.exe', version: 'git version 2.51.0.windows.1' },
          { tool: 'uv', ready: true, path: 'C:\\Wan2GP\\.tools\\uv.exe', version: 'uv 0.9.2', managed: true },
          { tool: 'python', ready: false, path: null, version: null },
          { tool: 'conda', ready: false, path: null, version: null },
        ],
      }
    if (name === 'install_prerequisite')
      return { ok: true, success: true, ready: true }
    if (name === 'get_status' || name === 'status') return STATUS
    // memory_profile_apply returns success/applied, NOT just ok — the callers
    // branch on r.success, so a stub without it makes every save look failed.
    if (name === 'memory_profile_apply')
      return { ok: true, success: true, applied: Object.keys(args && args.settings || {}), snapshot: 'harness' }
    if (name === 'llm_engines_list') return ENGINES
    if (name === 'config_load') return CONFIG
    if (name === 'get_desktop_version') return '0.9.2'
    if (name === 'detect_gpu' || name === 'get_gpu_info') return STATUS.gpu
    if (name === 'auto_tune_recommend') {
        // Real shape (features.rs auto_tune_recommend), including the v17 keys,
        // so the Performance Settings rec/saved path is actually exercised.
        return {
          ok: true,
          // v18.20 P0: what a 10 GB / 31 GB box now gets (tight × very_low).
          video_profile: 0, image_profile: 0, audio_profile: 0,
          vram_safety_coefficient: 0.7, vae_config: 0,
          transformer_quantization: 'int8', int8_kernels: 'auto', kernel_precision: 'fast',
          vram_allocator: 'vmm_spill', attention_head_split: 2,
          smart_memory_pinning: true,
          video_preload_mode: 'default', image_preload_mode: 'default', audio_preload_mode: 'default',
          perc_reserved_mem_max: 0,
          _recommendation_label: 'LowRAM · no permanent Reserved RAM (upstream: experimental)',
          _recommendation_reason: 'Auto-tuned for your hardware',
          packages: ['torch', 'triton', 'sageattention'], kernels: ['nunchaku', 'gguf'],
        }
      }
    if (name === 'deepy_status') return { ok: true, available: true, mode: 'prime', deepyEnabled: true, deepyType: 'prime', currentEngine: 'opencode', promptEnhancer: true, enhancerEnabled: 4, promptEnhancerQuantization: 'gguf', sessionMode: 'selectable', sessionResetMode: 'reset_session', sessionGalleryMediaMode: 'copy', enhancerMode: 1, engines: [], profiles: {} }
    if (name === 'plugins_list') return { ok: true, plugins: [] }
    if (name === 'config_backups_list') return { ok: true, backups: [] }
    if (name === 'upstream_changelog') return { ok: true, commits: [] }
    // Two rows so the Delete action column is exercised (root + adult-pack).
    if (name === 'library_models') return {
      ok: true, root: 'C:\\Wan2GP\\models', files: 2,
      bytes: 32 * 1024 ** 3 + 7 * 1024 ** 3, truncated: false,
      sample: [
        { name: 'Minimax-H3-FL2VA int8_convrot.safetensors', rel: 'Minimax-H3-FL2VA int8_convrot.safetensors', bytes: 32 * 1024 ** 3, kind: 'INT8', origin: 'DeepBeepMeep' },
        { name: 'pack_model_int8.safetensors', rel: 'adult-pack/pack_model_int8.safetensors', bytes: 7 * 1024 ** 3, kind: 'INT8', origin: 'Pack' },
      ],
    }
    if (name === 'library_model_delete') return { ok: true, rel: args && args.rel, bytes: 0 }
    if (name === 'library_loras') return { ok: true, root: 'C:\\Wan2GP\\loras', folders: [] }
    if (name === 'library_finetunes') return { ok: true, dir: 'C:\\Wan2GP\\finetunes', items: [] }
    if (name === 'workspace_list') return { ok: true, dir: 'C:\\Wan2GP\\workspaces', items: [], archived: 0 }
    if (name === 'get_wangp_version') return '1.3.5'
    if (name === 'triton_status' || name === 'long_paths_status') return { ok: true, enabled: true }
    if (name === 'dlss5_classify') return { ok: true, tier: 'supported', found: true }
    if (name === 'log_server_status') return { ok: true, running: false, port: 7862, lan: false, samePc: null, phone: null, phoneUnavailable: true }
    if (name === 'deepy_web_status') return { ok: true, running: false, mode: 'disabled' }
    // Disk cleanup panel: the real shapes (sizes are only present when the
    // caller asked, mirroring cleanup_scan(withSizes)).
    if (name === 'cleanup_scan') {
      var sized = !!(args && args.withSizes)
      var CLEAN = [
        { key: 'gradio', label: 'Gradio temp', path: 'C:\\Users\\me\\AppData\\Local\\Temp\\gradio', exists: true, bytes: 558799128, files: 24, risky: false, warning: 'Uploads and previews Gradio copies while Wan2GP runs.' },
        { key: 'uv', label: 'uv wheel cache', path: 'C:\\Wan2GP\\.uv-cache', exists: true, bytes: 9696516823, files: 61513, risky: false, warning: 'Wheels for your Python environment.' },
        { key: 'wheels', label: 'Launcher wheel cache', path: 'C:\\Wan2GP\\wheel-cache', exists: true, bytes: 308121043, files: 3, risky: false, warning: 'Pinned kernel wheels.' },
        { key: 'triton', label: 'Triton JIT cache', path: 'C:\\Wan2GP\\triton-cache', exists: true, bytes: 172229976, files: 3897, risky: false, warning: 'Compiled Triton kernels.' },
        { key: 'pip', label: 'pip cache', path: 'C:\\Users\\me\\AppData\\Local\\pip\\cache', exists: true, bytes: 77375650680, files: 8072, risky: false, warning: "pip's download cache, shared by every Python project on this machine." },
        { key: 'hf', label: 'HuggingFace hub cache', path: 'C:\\Users\\me\\.cache\\huggingface\\hub', exists: true, bytes: 71860245251, files: 134, risky: true, warning: 'Model weights fetched through huggingface_hub live ONLY here (Deepy LLM engines, Kandinsky, some VAE/ONNX files). Deleting them means downloading them again — tens of GB. Models in your Wan2GP model folder (ckpts) are NOT affected, and your HF login token is kept.' },
      ]
      return { ok: true, targets: sized ? CLEAN : CLEAN.map(function (t) { return Object.assign({}, t, { bytes: 0, files: 0 }) }) }
    }
    if (name === 'cleanup_clean') {
      var keys = (args && args.keys) || []
      return { ok: true, freedBytes: 558799128, removedFiles: 24, cleared: keys, errors: [], targets: [] }
    }
    if (name === 'get_disk_space') return { ok: true, free: 512 * 1024 * 1024 * 1024, total: 1024 * 1024 * 1024 * 1024 }
    // v17 RAM/VRAM troubleshooting. Stubbed to a real RTX_30 answer so the
    // known-good panel can be driven in the browser; the OOM remedy returns
    // the same applied-list shape the real command does.
    if (name === 'troubleshoot_known_good')
      return { ok: true, profileKey: 'RTX_30', recipe: { settings: { attention_mode: 'sage2', video_profile: 3, compile: true }, note: 'Needs SageAttention 2.2.0 (Sync GPU Wheels); Tea Cache 2.0 in WanGP' } }
    if (name === 'troubleshoot_oom_remedy') {
        var REMEDY = { head_split_medium: { attention_head_split: 2 }, lower_reserved_ram: { perc_reserved_mem_max: 25, smart_memory_pinning: true } }
        var set = REMEDY[(args && args.action) || ''] || {}
        var r = payload('memory_profile_apply', { settings: set })
        r.remedy = (args && args.action) || ''
        return r
      }
    if (name === 'troubleshoot_vram_diag')
      return { ok: true, script: (args && args.action) === 'trim' ? 'gputrim.cmd' : 'gpumem.cmd', exit: 0, output: 'PID  Name          MB\n 4828  python.exe   5120\n\n8192 MiB used / 10240 MiB total' }
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
