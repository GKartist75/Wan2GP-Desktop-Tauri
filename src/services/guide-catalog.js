/**
 * guide-catalog.js — curated WanGP model recommendations for the launcher Guide tab.
 *
 * Pure, side-effect-free data module (no Tauri, no Node, no DOM). Mirrors the
 * starters in docs/WAN2GP-GUIDE.md; every model_type must exist in WanGP's
 * defaults/*.json (verified 2026-09-21: 25/25 present).
 *
 * Shape per goal: { id, label, picks: [{ model, why, frames, steps, guidance, tip }] }
 * Frames/steps/guidance are starters, not rules — the per-model form owns truth.
 * Accelerator LoRAs always want guidance 1 (the #1 bad-quality cause otherwise).
 */

'use strict'

const GUIDE_GOALS = [
  {
    id: 'first-video',
    label: 'First video (text → video)',
    picks: [
      { model: 't2v_1.3B', why: 'Fastest, ~6 GB VRAM. Start here.', frames: 49, steps: 20, guidance: 5, tip: 'Prompt: subject + action + setting + style, e.g. "A cat walking in a garden, cinematic, soft light".' },
      { model: 't2v_2_2', why: 'General 14B quality when you have 12 GB+.', frames: 49, steps: 20, guidance: 4, tip: 'Same prompt style; raise steps to 25-30 for final quality.' }
    ]
  },
  {
    id: 'animate-photo',
    label: 'Animate a photo (image → video)',
    picks: [
      { model: 'i2v_2_2', why: 'Mature general image-to-video.', frames: 49, steps: 20, guidance: 5, tip: 'Start Image holds the subject — prompt only motion/camera: "she smiles, turns toward the window, camera pushes in".' },
      { model: 'minimax_h3_fl2va_pruned', why: 'Realistic motion + synced audio in one pass.', frames: 124, steps: 15, guidance: 5, tip: 'Needs 15-20 steps minimum (no distilled ckpt yet).' }
    ]
  },
  {
    id: 'cinematic-audio',
    label: 'Cinematic video with native audio',
    picks: [
      { model: 'ltx2_25_22B_distilled', why: 'Fast video + soundtrack one pass; start/end frames, control, outpaint.', frames: 97, steps: 8, guidance: 1, tip: 'Distilled default; Dev variant trades speed for control.' }
    ]
  },
  {
    id: 'talking-head',
    label: 'Talking head / dialogue',
    picks: [
      { model: 'longcat_avatar_v1_5', why: 'Distilled audio-driven avatar, sliding windows for length.', frames: 97, steps: 8, guidance: 1, tip: 'Audio carries voice identity; keep dialogue in ONE prompt (FG mode).' },
      { model: 'infinitetalk', why: 'Long multi-speaker dialogue, window-chained coherence.', frames: 97, steps: 20, guidance: 5, tip: 'One prompt line per story beat in W/PW window modes.' }
    ]
  },
  {
    id: 'edit-video',
    label: 'Edit / replace / outpaint video',
    picks: [
      { model: 'vace_14B_fusionix', why: 'Accelerated VACE default (~8-10 steps).', frames: 49, steps: 10, guidance: 1, tip: 'Control Video + Matanyone mask + reference image; grey 127 = replace.' },
      { model: 'vace_14B', why: 'Original CFG VACE, max quality.', frames: 49, steps: 30, guidance: 5, tip: 'Enable Skip Layer Guidance; describe the whole scene, esp. background.' }
    ]
  },
  {
    id: 'face-swap',
    label: 'Face replacement (keep identity)',
    picks: [
      { model: 'lynx', why: 'Identity-preserving face swap under new hair/clothes/light.', frames: 49, steps: 20, guidance: 5, tip: 'Feed reference portraits; blend with a VACE pass for background cleanup.' }
    ]
  },
  {
    id: 'motion-transfer',
    label: 'Motion transfer / performer replace',
    picks: [
      { model: 'animate2', why: 'Pose/control video → character follows movement & camera.', frames: 81, steps: 15, guidance: 4, tip: 'Align reference/start image to the control first frame.' },
      { model: 'scail2_14B', why: 'Multi-person animate/replace with sliding windows.', frames: 81, steps: 15, guidance: 4, tip: 'Colored masks for 2+ people (Magic Mask).' }
    ]
  },
  {
    id: 'image',
    label: 'General image',
    picks: [
      { model: 'z_image', why: 'Fast 6B iteration for posters, key art, i2v sources.', frames: 0, steps: 8, guidance: 1, tip: 'Turbo distilled; iterate fast, upscale later.' },
      { model: 'krea2_turbo', why: 'Polished/aesthetic + world knowledge (people, landmarks).', frames: 0, steps: 12, guidance: 4, tip: 'RAW checkpoint for CFG control; Turbo for speed.' }
    ]
  },
  {
    id: 'image-edit',
    label: 'Image edit / identity',
    picks: [
      { model: 'qwen_image_edit_plus_20B', why: 'Multi-subject combine + long rendered text.', frames: 0, steps: 30, guidance: 4, tip: 'Instruction verbs: add/remove/replace/change — keep face/scene explicitly.' },
      { model: 'krea2_turbo_edit', why: 'Up to 2 refs + inpaint/outpaint built in.', frames: 0, steps: 12, guidance: 4, tip: 'Good for keyframes before video runs.' }
    ]
  },
  {
    id: 'poster-text',
    label: 'Poster / infographic (text matters)',
    picks: [
      { model: 'ideogram4', why: 'Layout + typography; Magic Prompt + visual helper for JSON prompts.', frames: 0, steps: 20, guidance: 5, tip: 'Use the wand helper to place text boxes; Turbo 4-8 steps for drafts.' },
      { model: 'sensenova_u1_5_8b_mot', why: 'Native-4K infographics, exact quoted wording.', frames: 0, steps: 8, guidance: 1, tip: 'Put exact wording in quotes; 8-step LoRA profile.' }
    ]
  },
  {
    id: 'speech',
    label: 'Speech / voice clone / dialogue',
    picks: [
      { model: 'qwen3_tts_base', why: 'Flexible baseline: clone, 2-speaker, modest VRAM.', frames: 0, steps: 20, guidance: 4, tip: 'Prompt = script; audio input = voice identity.' },
      { model: 'index_tts2', why: 'Expressive emotion + very long conversations.', frames: 0, steps: 20, guidance: 4, tip: 'Speaker 1:/Speaker 2: tags; keep script together (FG).' }
    ]
  },
  {
    id: 'song',
    label: 'Song with lyrics',
    picks: [
      { model: 'ace_step_v1_5_xl', why: 'Lyric adherence + full tracks.', frames: 0, steps: 20, guidance: 4, tip: 'Lyrics in prompt with [Verse]/[Chorus]; style in tags field.' },
      { model: 'minimax_music3', why: 'Complete 5-min stereo songs (needs ~16 GB for fast profiles).', frames: 0, steps: 20, guidance: 4, tip: 'Audio profile 3 engages the fast LM decoder.' }
    ]
  },
  {
    id: 'sfx',
    label: 'Ambience / sound effects',
    picks: [
      { model: 'stable_audio3_small', why: 'Music loops, ambience, SFX from description.', frames: 0, steps: 20, guidance: 4, tip: 'Or add sound to silent video later via MMAudio post.' }
    ]
  }
]

/**
 * Display names from WanGP defaults/*.json (model.name). The toolbar search
 * matches these — copying the id (e.g. t2v_2_2) is useless there, so the UI
 * copies the full name. Verified 2026-09-21.
 */
const MODEL_NAMES = {
  't2v_1.3B': 'Wan2.1 Text2video 1.3B',
  t2v_2_2: 'Wan2.2 Text2video 14B',
  i2v_2_2: 'Wan2.2 Image2video 14B',
  minimax_h3_fl2va_pruned: 'MiniMax H3 FL2VA Pruned 20B',
  ltx2_25_22B_distilled: 'LTX-2 2.5 Distilled 22B',
  longcat_avatar_v1_5: 'LongCat Avatar 1.5 Distilled 13.6B',
  infinitetalk: 'Infinitetalk Single Speaker 480p 14B',
  vace_14B_fusionix: 'Vace FusioniX 14B',
  vace_14B: 'Vace 14B',
  lynx: 'Wan2.1 Lynx 14B',
  animate2: 'Wan2.2 Animate 2 14B',
  scail2_14B: 'SCAIL-2 14B',
  z_image: 'Z-Image Turbo 6B',
  krea2_turbo: 'Krea 2 Turbo',
  qwen_image_edit_plus_20B: 'Qwen Image Edit Plus (2509) 20B',
  krea2_turbo_edit: 'Krea 2 Turbo Identity Edit v1.2',
  ideogram4: 'Ideogram v4 FP8 9.3B',
  sensenova_u1_5_8b_mot: 'SenseNova U1.5 8B MoT',
  qwen3_tts_base: 'TTS Qwen3 Base (12Hz) 1.7B',
  index_tts2: 'TTS Index TTS 2',
  ace_step_v1_5_xl: 'Music ACE-Step v1.5 XL Turbo 4B',
  minimax_music3: 'Music MiniMax Music 3',
  stable_audio3_small: 'Music Stable Audio 3 Small Music'
}

/**
 * Full display name for a pick (falls back to the id when unmapped).
 * @param {{model:string}} pick
 */
function pickName(pick) {
  const n = MODEL_NAMES[pick.model]
  return n ? n : pick.model
}
function guideGoal(goalId) {
  return GUIDE_GOALS.find((g) => g.id === goalId) || null
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = { GUIDE_GOALS, MODEL_NAMES, guideGoal, pickName }
}
