/**
 * VRAM / RAM Adjuster — manual memory-profile overrides for Wan2GP's wgp_config.json.
 *
 * This is the desktop-side counterpart to the Auto-Tune panel: instead of
 * detecting hardware and recommending, the user sets the memory knobs directly
 * and we write them to wgp_config.json. Pure, offline-testable module (no
 * Electron deps), matching the conventions of services/auto-tune.js.
 *
 * The writable keys are exactly Wan2GP's mmgp-related wgp_config.json keys
 * (see services/auto-tune.js appliedKeys()). There is NO reserved-memory key
 * in wgp_config.json — mmgp reserved memory is a CLI arg (--perc-reserved-mem-max),
 * which the launcher already forwards elsewhere; this panel owns only the config keys.
 */

'use strict'

const fs = require('fs')
const path = require('path')

// Keys this panel is allowed to touch (subset of auto-tune appliedKeys,
// plus the upstream v13.13 kernel settings).
const MEMORY_KEYS = [
  'video_profile',
  'image_profile',
  'audio_profile',
  'vram_safety_coefficient',
  'vae_config',
  'transformer_quantization',
  'int8_kernels',
  'kernel_precision'
]

// Valid mmgp profile numbers (1..5, with the 3.5/4.5 half-steps Wan2GP supports).
const VALID_PROFILES = [1, 2, 3, 3.5, 4, 4.5, 5]
// vae_config choices in Wan2GP: 0=auto, 1=16GB+, 2=8GB+, 3=6GB+
// (Qwen Image 2.1: 1024/512/256px tiles with 25% overlap, Auto thresholds
// 16000/8000 MiB; older Wan VAEs used full/untiled + fixed tile sizes).
const VALID_VAE = [0, 1, 2, 3]
const VALID_QUANT = ['none', 'int8', 'fp8', 'nvfp4']
// Upstream v13.13: shared/kernels/int8_backend.py CHOICES and
// shared/kernels/kernel_policy.py CHOICES ("strict"/"fast").
const VALID_INT8_KERNELS = ['auto', 'disabled', 'triton', 'kitchen']
const VALID_KERNEL_PRECISION = ['fast', 'strict']

function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)) }

/**
 * Schema validation for a proposed override object.
 * Returns { ok, errors: [{field,message}] }.
 */
function validateMemorySettings(s) {
  const errors = []
  if (s == null || typeof s !== 'object') {
    return { ok: false, errors: [{ field: '(root)', message: 'settings must be an object' }] }
  }
  for (const key of MEMORY_KEYS) {
    if (!(key in s)) continue
    const v = s[key]
    if (key.endsWith('_profile')) {
      if (typeof v !== 'number' || !VALID_PROFILES.includes(v)) {
        errors.push({ field: key, message: `must be one of ${VALID_PROFILES.join(', ')}` })
      }
    } else if (key === 'vram_safety_coefficient') {
      const n = Number(v)
      if (!isFinite(n) || n <= 0 || n > 1) {
        errors.push({ field: key, message: 'must be a number in (0, 1]' })
      }
    } else if (key === 'vae_config') {
      if (!VALID_VAE.includes(Number(v))) {
        errors.push({ field: key, message: `must be one of ${VALID_VAE.join(', ')}` })
      }
    } else if (key === 'transformer_quantization') {
      if (!VALID_QUANT.includes(v)) {
        errors.push({ field: key, message: `must be one of ${VALID_QUANT.join(', ')}` })
      }
    } else if (key === 'int8_kernels') {
      if (!VALID_INT8_KERNELS.includes(v)) {
        errors.push({ field: key, message: `must be one of ${VALID_INT8_KERNELS.join(', ')}` })
      }
    } else if (key === 'kernel_precision') {
      if (!VALID_KERNEL_PRECISION.includes(v)) {
        errors.push({ field: key, message: `must be one of ${VALID_KERNEL_PRECISION.join(', ')}` })
      }
    }
  }
  return { ok: errors.length === 0, errors }
}

/**
 * Read current memory settings from wgp_config.json (only MEMORY_KEYS).
 * Missing keys are reported as null so the UI can show "unset".
 */
function readMemorySettings(repoDir, dataDir) {
  // Mirror auto-tune's resolution order.
  const candidates = [
    path.join(repoDir || '', 'wgp_config.json'),
    path.join(dataDir || '', 'wgp_config.json'),
    path.join(dataDir || '', 'Wan2GP', 'wgp_config.json')
  ]
  let config = {}
  for (const p of candidates) {
    if (p && fs.existsSync(p)) {
      try { config = JSON.parse(fs.readFileSync(p, 'utf8')); break } catch { /* try next */ }
    }
  }
  const out = {}
  for (const key of MEMORY_KEYS) out[key] = (key in config) ? config[key] : null
  return out
}

/**
 * Apply memory overrides, merging into the existing wgp_config.json so we never
 * clobber unrelated keys (Auto-Tune's other settings, model paths, etc.).
 */
function applyMemorySettings(settings, repoDir, dataDir) {
  const v = validateMemorySettings(settings)
  if (!v.ok) {
    return { success: false, error: v.errors.map((e) => `${e.field}: ${e.message}`).join('; '), applied: [] }
  }
  const candidates = [
    path.join(repoDir || '', 'wgp_config.json'),
    path.join(dataDir || '', 'wgp_config.json'),
    path.join(dataDir || '', 'Wan2GP', 'wgp_config.json')
  ]
  let cfgPath = null
  let config = {}
  for (const p of candidates) {
    if (p && fs.existsSync(p)) { cfgPath = p; try { config = JSON.parse(fs.readFileSync(p, 'utf8')) } catch { config = {} } break }
  }
  if (!cfgPath) cfgPath = candidates[0]

  const applied = []
  for (const key of MEMORY_KEYS) {
    if (key in settings && settings[key] !== undefined && settings[key] !== null && settings[key] !== '') {
      // Normalize numeric keys.
      config[key] = (key === 'vram_safety_coefficient') ? Number(settings[key]) : settings[key]
      applied.push(key)
    }
  }
  if (!config.services) config.services = {}
  // Manual edit — clear the "auto applied" flag so Auto-Tune won't silently override.
  config.services.auto_performance_applied = false

  fs.mkdirSync(path.dirname(cfgPath), { recursive: true })
  fs.writeFileSync(cfgPath, JSON.stringify(config, null, 2), 'utf8')
  return { success: true, path: cfgPath, applied }
}

module.exports = {
  MEMORY_KEYS,
  VALID_PROFILES,
  VALID_VAE,
  VALID_QUANT,
  VALID_INT8_KERNELS,
  VALID_KERNEL_PRECISION,
  validateMemorySettings,
  readMemorySettings,
  applyMemorySettings
}
