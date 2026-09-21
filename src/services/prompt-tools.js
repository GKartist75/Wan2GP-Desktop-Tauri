/**
 * prompt-tools.js — pure helpers for Guide → Prompt tools (F5).
 *
 * Sliding-window `[/...]` command validation per docs/PROMPTS.md plus a small
 * template gallery. No Tauri, no Node, no DOM. Copy to .cjs for node checks.
 */

'use strict'

/** Starter templates: { id, label, mode, text }. Mode is the line-processing choice. */
const PROMPT_TEMPLATES = [
  {
    id: 't2v-scene',
    label: 'Text → video scene',
    mode: 'G — each line a new queued job',
    text: 'A red sports car driving through a mountain road at sunset, cinematic, high quality'
  },
  {
    id: 'i2v-motion',
    label: 'Image → video motion',
    mode: 'G — each line a new queued job',
    text: 'the woman smiles, turns toward the window, and the camera slowly pushes in'
  },
  {
    id: 'edit-instruction',
    label: 'Instruction edit (Qwen Edit / Kontext / Chrono)',
    mode: 'FG — all lines one prompt',
    text: 'Add a red wool hat to the woman, keep her face, hairstyle, and the rainy street unchanged.'
  },
  {
    id: 'dialogue',
    label: 'Two-speaker dialogue (TTS / InfiniteTalk)',
    mode: 'FG — all lines one prompt',
    text: 'Speaker 1: We should leave before the rain gets heavier.\nSpeaker 2: Give me one minute, I still need my jacket.'
  },
  {
    id: 'lyrics',
    label: 'Song lyrics (ACE-Step / HeartMuLa)',
    mode: 'FG — all lines one prompt',
    text: '[Verse]\nMorning light through the window pane\n[Chorus]\nStay with me through every mile'
  },
  {
    id: 'windows-story',
    label: 'Long video beats (one line per window)',
    mode: 'W — each line a new sliding window',
    text: '[/duration=25%] A wide dawn shot of a mountain train station.\n[/duration=5s,/overlap=17] The violinist steps onto the train.\n[/new_shot,/duration=4s] A sharp cut to inside the dining car at night.'
  }
]

const KNOWN_COMMANDS = new Set([
  'duration', 'overlap', 'new_shot', 'no_end_image', 'loras_mult',
  'store_mem', 'load_mem', 'drop_mem'
])

/**
 * Validate `[/...]` window commands in a prompt.
 * @param {string} text full prompt text
 * @returns {Array<{line:number, message:string}>} issues (empty = clean)
 */
function validateWindowCommands(text) {
  const issues = []
  const lines = String(text == null ? '' : text).split('\n')
  lines.forEach((line, i) => {
    const re = /\[(\/[^\]]*)\]/g
    let m
    while ((m = re.exec(line)) !== null) {
      // Combined form is [/duration=5s,/overlap=9] — only the first command
      // carries the slash (per PROMPTS.md), so strip one leading slash total.
      const inner = m[1].replace(/^\//, '')
      const parts = inner.split(',').map((s) => s.trim().replace(/^\//, '')).filter(Boolean)
      if (!parts.length) {
        issues.push({ line: i + 1, message: 'Empty [/...] command' })
        continue
      }
      for (const part of parts) {
        const name = part.split('=')[0].trim()
        if (!KNOWN_COMMANDS.has(name)) {
          issues.push({ line: i + 1, message: 'Unknown window command "/' + name + '" — windows ignore unknown [/...] at validation' })
          continue
        }
        if (name === 'duration') {
          const v = (part.split('=')[1] || '').trim()
          if (!/^\d+$/.test(v) && !/^\d+(\.\d+)?s$/.test(v) && !/^\d+(\.\d+)?%$/.test(v)) {
            issues.push({ line: i + 1, message: 'Bad [/duration=' + v + '] — use frames (121), seconds (5s) or percent (20%)' })
          }
        }
        if (name === 'overlap') {
          const v = (part.split('=')[1] || '').trim()
          if (v !== '' && !/^\d+$/.test(v)) {
            issues.push({ line: i + 1, message: 'Bad [/overlap=' + v + '] — use a frame count or bare [/overlap]' })
          }
        }
      }
    }
  })
  return issues
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = { PROMPT_TEMPLATES, KNOWN_COMMANDS, validateWindowCommands }
}
