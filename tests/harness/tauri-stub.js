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
          video_profile: 4, image_profile: 4, audio_profile: 4,
          vram_safety_coefficient: 0.7, vae_config: 0,
          transformer_quantization: 'int8', int8_kernels: 'auto', kernel_precision: 'fast',
          vram_allocator: 'vmm_spill', attention_head_split: 2,
          smart_memory_pinning: true,
          video_preload_mode: 'default', image_preload_mode: 'dynamic', audio_preload_mode: 'default',
          perc_reserved_mem_max: 0,
          _recommendation_label: 'LowRAM · LowVRAM (upstream: recommended)',
          _recommendation_reason: 'Auto-tuned for your hardware',
          packages: ['torch', 'triton', 'sageattention'], kernels: ['nunchaku', 'gguf'],
        }
      }
    if (name === 'deepy_status') return { ok: true, available: true, mode: 'prime', deepyEnabled: true, deepyType: 'prime', currentEngine: 'opencode', promptEnhancer: true, enhancerEnabled: 4, promptEnhancerQuantization: 'gguf', sessionMode: 'selectable', sessionResetMode: 'reset_session', sessionGalleryMediaMode: 'copy', enhancerMode: 1, engines: [], profiles: {} }
    if (name === 'plugins_list') return { ok: true, plugins: [] }
    if (name === 'config_backups_list') return { ok: true, backups: [] }
    if (name === 'upstream_changelog') return { ok: true, commits: [] }
    if (name === 'library_models') return { ok: true, root: 'C:\\Wan2GP\\models', files: 0, bytes: 0, truncated: false, sample: [] }
    if (name === 'library_loras') return { ok: true, root: 'C:\\Wan2GP\\loras', folders: [] }
    if (name === 'library_finetunes') return { ok: true, dir: 'C:\\Wan2GP\\finetunes', items: [] }
    if (name === 'workspace_list') return { ok: true, dir: 'C:\\Wan2GP\\workspaces', items: [], archived: 0 }
    if (name === 'get_wangp_version') return '1.3.5'
    if (name === 'triton_status' || name === 'long_paths_status') return { ok: true, enabled: true }
    if (name === 'dlss5_classify') return { ok: true, tier: 'supported', found: true }
    if (name === 'log_server_status') return { ok: true, running: false, port: 7862, lan: false, samePc: null, phone: null, phoneUnavailable: true }
    if (name === 'uv_cache_info') return { ok: true, sizeGb: 0 }
    if (name === 'deepy_web_status') return { ok: true, running: false, mode: 'disabled' }
    // Adult (NSFW) model pack. The manifest is served from src/ by the harness
    // itself, so the panel renders against the REAL catalog instead of a
    // hand-made fixture — a broken manifest must be visible here too.
    if (name === 'adult_pack_manifest') {
      return fetch('/adult-pack-manifest.json')
        .then(function (r) { return r.json() })
        .then(function (m) {
          return {
            ok: true,
            manifestVersion: m.manifestVersion,
            generatedAt: m.generatedAt,
            verifiedAgainst: m.verifiedAgainst,
            title: m.title,
            groups: m.groups,
            archMap: m.archMap || [],
            legal: m.legal,
            totals: m.totals,
            loras: (m.loras || []).map(function (l) { return Object.assign({}, l, { installed: false }) }),
            finetunes: (m.finetunes || []).map(function (f) {
              return Object.assign({}, f, { architectureAvailable: true, finetuneWritten: false })
            }),
            civitai: m.civitai || [],
            paths: {
              loras: 'D:\\Wan2GP-Models\\loras\\adult-pack',
              ckpts: 'D:\\Wan2GP-Models\\ckpts',
              finetunes: 'D:\\Wan2GP\\finetunes',
            },
            wan2gpModelCount: 270,
            ageConfirmedAt: null,
            hasToken: false,
            savedSelections: {},
          }
        })
    }
    if (name === 'adult_pack_save') return { ok: true }
    if (name === 'adult_pack_updates') {
      return {
        ok: true,
        checkedAt: '1760000000',
        reposChecked: 34,
        items: [
          {
            id: 'wan_rapid_aio-Q4_K_M', name: 'Wan 2.2 Rapid AllInOne NSFW', repo: 'lopi999/x', path: 'a.safetensors',
            half: 'high', status: 'updated', liveSha: 'aa'.repeat(32), liveBytes: 2147483648,
            pinnedSha: 'bb'.repeat(32), pinnedBytes: 2097152000, effectiveSha: 'bb'.repeat(32),
            effectiveBytes: 2097152000, deltaBytes: 50331648, overridden: false,
            installed: false, installedStale: false, bytesOnDisk: 0,
            repoLastModified: '2026-10-02T00:00:00.000Z', action: 'download',
          },
          {
            id: 'ltx23-cumshot', name: 'LTX 2.3 Cumshot', repo: 'lopi999/y', path: 'b.safetensors',
            half: 'high', status: 'updated', liveSha: 'cc'.repeat(32), liveBytes: 1073741824,
            pinnedSha: 'dd'.repeat(32), pinnedBytes: 1073741824, effectiveSha: 'dd'.repeat(32),
            effectiveBytes: 1073741824, deltaBytes: 0, overridden: false,
            installed: true, installedStale: false, bytesOnDisk: 1073741824,
            repoLastModified: '2026-09-28T00:00:00.000Z', action: 'none',
          },
          {
            id: 'h3_after_midnight', name: 'AfterMidnight H3 v1.2', repo: 'lopi999/z', path: 'c.safetensors',
            half: 'high', status: 'updated', liveSha: 'ee'.repeat(32), liveBytes: 3221225472,
            pinnedSha: 'ff'.repeat(32), pinnedBytes: 3000000000, effectiveSha: 'ff'.repeat(32),
            effectiveBytes: 3000000000, deltaBytes: 221225472, overridden: false,
            installed: true, installedStale: true, bytesOnDisk: 3000000000,
            repoLastModified: '2026-10-11T00:00:00.000Z', action: 'update',
          },
          {
            id: 'img_zimage', name: 'Z-Image NSFW', repo: 'lopi999/w', path: 'd.safetensors',
            half: 'high', status: 'current', liveSha: 'ab'.repeat(32), liveBytes: 100,
            pinnedSha: 'ab'.repeat(32), pinnedBytes: 100, effectiveSha: 'ab'.repeat(32),
            effectiveBytes: 100, deltaBytes: 0, overridden: false,
            installed: true, installedStale: false, bytesOnDisk: 100,
            repoLastModified: '2026-10-01T00:00:00.000Z', action: 'none',
          },
        ],
        failed: [],
      }
    }
    if (name === 'adult_pack_apply_updates') {
      return {
        ok: true,
        applied: (args.ids || []).map(function (id) { return { id: id, sha256: 'aa'.repeat(32), bytes: 1 } }),
        skipped: [],
        pins: {},
      }
    }
    // Galleries return REAL sample media (a Wan webp + an H3 sample clip), so
    // the browser harness exercises the actual grid/video/lightbox rendering
    // instead of an empty drawer.
    if (name === 'adult_pack_gallery') {
      return {
        ok: true, repo: args && args.repo, rev: (args && args.rev) || 'main', pinned: true,
        images: [
          { url: 'https://huggingface.co/lopi999/Wan2.2-I2V_General-NSFW-LoRA/resolve/main/images/WanVideo2_2_I2V_00205.webp', name: 'WanVideo2_2_I2V_00205.webp' },
          { url: 'https://huggingface.co/lynaNSFW/minimaxH3_Collection/resolve/main/images/undefined_image%20(1).png', name: 'undefined_image (1).png' },
        ],
        videos: [
          { url: 'https://huggingface.co/SexGod1979/PinkCherry-NSFW_MiniMax-H3/resolve/main/1.8-hybrid/TEST__00211-audio.mp4', name: 'TEST__00211-audio.mp4' },
        ],
      }
    }
    if (name === 'adult_pack_civitai_gallery') {
      return { ok: true, name: 'example', images: [{ url: 'https://image.civitai.com/example.jpg', nsfw: true, width: 1024, height: 1024 }] }
    }
    // Search / inspect / add, so the "find your own models" flow can be driven
    // in the browser without hitting the network.
    if (name === 'adult_pack_search') {
      if (args && args.source === 'civitai') {
        return {
          ok: true, source: 'civitai', query: args && args.query, count: 2,
          items: [
            {
              source: 'civitai', id: 1899045, title: 'WAN 2.2 I2V - Combo Handjob Blowjob',
              baseModel: 'Wan 2.2', downloads: 41230, likes: 980, versionName: 'HIGH V1',
              releasedAt: '2025-08-26T02:20:09.949Z', archLabel: 'Wan 2.2 (image to video)',
              family: 'wan_i2v', architecture: 'i2v_2_2', wan2gpIds: ['i2v_2_2'],
              page: 'https://civitai.red/models/1899045',
              examples: [
                { url: 'https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/693097eb/original=true/handjob.mp4', kind: 'video' },
                { url: 'https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/310c6ead/original=true/blowjob.png', kind: 'image' },
              ],
            },
            {
              source: 'civitai', id: 2837571, title: 'Minimax H3 Turbo LoRas',
              baseModel: 'MiniMax H3', downloads: 12000, likes: 300, versionName: 'v2',
              releasedAt: '2026-08-11T00:00:00.000Z', archLabel: 'MiniMax H3 (reference to video/audio, pruned)',
              family: 'h3', architecture: 'minimax_h3_ref2va_pruned', wan2gpIds: ['minimax_h3_ref2va_pruned'],
              page: 'https://civitai.red/models/2837571',
              examples: [
                { url: 'https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/a1b2c3d4/original=true/h3.png', kind: 'image' },
                { url: 'https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/e5f6a7b8/original=true/h3b.png', kind: 'image' },
              ],
            },
          ],
        }
      }
      return {
        ok: true, source: args && args.source, query: args && args.query, count: 2,
        items: [
          { source: 'hf', id: 'example/minimax-h3-nsfw', title: 'example/minimax-h3-nsfw', downloads: 12345, likes: 67, archLabel: 'MiniMax H3 (reference to video/audio, pruned)', architecture: 'minimax_h3_ref2va_pruned', wan2gpIds: ['minimax_h3_ref2va_pruned'], repoPage: 'https://huggingface.co/example/minimax-h3-nsfw' },
          { source: 'hf', id: 'example/wan22-lora', title: 'example/wan22-lora', downloads: 890, likes: 4, archLabel: 'Wan LoRA (any Wan 2.x)', architecture: 'i2v_2_2', wan2gpIds: ['i2v_2_2'], repoPage: 'https://huggingface.co/example/wan22-lora' },
        ],
      }
    }
    if (name === 'adult_pack_browse') {
      return {
        ok: true, source: args && args.source, sort: args && args.sort, period: args && args.period, count: 2,
        items: [
          {
            source: 'civitai', id: 2867841, title: 'SexGod "After Midnight" MM H3 ref2va',
            baseModel: 'MiniMax H3', downloads: 88400, likes: 2100, versionName: 'v1.2',
            releasedAt: '2026-09-07T10:00:00.000Z', archLabel: 'MiniMax H3 (reference to video/audio, pruned)',
            family: 'h3', architecture: 'minimax_h3_ref2va_pruned', wan2gpIds: ['minimax_h3_ref2va_pruned'],
            page: 'https://civitai.red/models/2867841',
            examples: [
              { url: 'https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/b1c2d3e4/original=true/am1.mp4', kind: 'video' },
              { url: 'https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/b1c2d3e4/original=true/am2.mp4', kind: 'video' },
            ],
          },
          {
            source: 'civitai', id: 2731589, title: 'LTX 2.3 - Furry 2D NSFW Multi Purpose Lora + Cum',
            baseModel: 'LTX-2.3', downloads: 55900, likes: 1400, versionName: 'v1.0',
            releasedAt: '2026-04-02T00:00:00.000Z', archLabel: 'LTX 2.3 (image to video)',
            family: 'ltx', architecture: 'ltxv_2_3', wan2gpIds: ['ltxv_2_3'],
            page: 'https://civitai.red/models/2731589',
            examples: [
              { url: 'https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/c1d2e3f4/original=true/ltx1.png', kind: 'image' },
            ],
          },
        ],
      }
    }
    if (name === 'adult_pack_inspect') {
      return {
        ok: true, source: 'hf', id: args && args.id, title: (args && args.id) || 'model',
        repoPage: 'https://huggingface.co/example/x', architecture: args && args.architecture,
        architectureKnown: true,
        preview: 'https://huggingface.co/lopi999/Wan2.2-I2V_General-NSFW-LoRA/resolve/main/images/WanVideo2_2_I2V_00205.webp',
        files: [
          { file: 'model_int8_convrot.safetensors', bytes: 20967668616, kind: 'checkpoint', pair: null, sha256: 'a'.repeat(64), url: 'https://huggingface.co/example/x/resolve/main/model_int8_convrot.safetensors' },
          { file: 'adapter_rank64.safetensors', bytes: 1192828320, kind: 'lora', pair: null, sha256: 'b'.repeat(64), url: 'https://huggingface.co/example/x/resolve/main/adapter_rank64.safetensors' },
        ],
        samples: [],
      }
    }
    if (name === 'adult_pack_add') return { ok: true, id: 'custom_hf-example', kind: 'checkpoint', custom: { loras: [], finetunes: [] } }
    // Finetune manager: built from the REAL manifest (as the backend does), plus a
    // hand-written local recipe so all three origins render. A hardcoded list
    // silently breaks for any id the harness does not know about — which looks
    // exactly like a product bug in the browser.
    if (name === 'adult_pack_finetune_list') {
      return fetch('/adult-pack-manifest.json')
        .then(function (r) { return r.json() })
        .then(function (m) {
          const items = (m.finetunes || []).map(function (f) {
            return {
              id: f.id, name: f.name, architecture: f.architecture,
              description: f.description, precisionToken: f.precisionToken,
              urlCount: (f.urls || []).length + (f.urls2 || []).length,
              bytes: f.bytes, written: false, origin: 'pack',
              path: 'D:\\Wan2GP\\finetunes\\adultpack_' + f.id + '.json',
              hasLocalWeights: false, localBytes: 0, needsToken: false, valid: true,
            }
          })
          items.push({
            id: 'my_own_model', name: 'My own merge', architecture: 't2v_2_2',
            urlCount: 2, written: true, origin: 'local',
            hasLocalWeights: false, localBytes: 0, valid: true,
          })
          return { ok: true, dir: 'D:\\Wan2GP\\finetunes', count: items.length, items: items }
        })
    }
    if (name === 'adult_pack_finetune_read') {
      return { ok: true, id: args.id, content: '{\n  "model": {\n    "name": "DaSiWa MiniMax H3",\n    "architecture": "minimax_h3_ref2va_pruned",\n    "URLs": [\n      "https://huggingface.co/jeuzu/Dasiwa_MiniMax_H3_V4_int8/resolve/main/DasiwaMinimaxH3_V4_int8.safetensors"\n    ]\n  },\n  "steps": 30\n}' }
    }
    if (name === 'adult_pack_finetune_write') return { ok: true, id: args.id, name: 'DaSiWa MiniMax H3', architecture: 'minimax_h3_ref2va_pruned', path: 'D:\\Wan2GP\\finetunes\\' + args.id + '.json' }
    if (name === 'adult_pack_finetune_check') {
      return { ok: true, id: args.id, results: [{ url: 'https://huggingface.co/jeuzu/x/resolve/main/y.safetensors', ok: true, status: 200 }] }
    }
    if (name === 'adult_pack_write_finetunes') {
      // The real command reports what it wrote, so the panel can say so; a
      // stub returning an empty list would make the happy path look like a
      // failure in the browser.
      const written = (args.ids || []).map(function (id) {
        return { id: id, path: 'D:\\Wan2GP\\finetunes\\adultpack_' + id + '.json' }
      })
      return { ok: true, written: written, skipped: [], dir: 'D:\\Wan2GP\\finetunes' }
    }
    if (name === 'adult_pack_fetch')
      return { ok: true, downloaded: 0, bytes: 0, done: [], failed: [] }
    if (name === 'adult_pack_civitai_verify') return { ok: false, error: 'no token' }
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
