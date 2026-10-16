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
      { model: 'minimax_h3_fl2va_pruned', why: 'Realistic motion + synced audio in one pass.', frames: 124, steps: 15, guidance: 5, tip: 'Needs 15-20 steps minimum (no distilled ckpt yet).' },
      { model: 'minimax_h3_fl2va_pruned_veda', why: 'Same settings/LoRAs as pruned FL2VA, up to 2x faster on long/high-res gens.', frames: 124, steps: 15, guidance: 5, tip: 'Needs Triton and a CC 8.0+ NVIDIA GPU; compare fine detail against the standard variant first.' }
    ]
  },
  {
    id: 'cinematic-audio',
    label: 'Cinematic video with native audio',
    picks: [
      { model: 'ltx2_25_22B_distilled', why: 'Fast video + soundtrack one pass; start/end frames, control, outpaint.', frames: 97, steps: 8, guidance: 1, tip: 'Distilled default; Dev variant trades speed for control.' },
      { model: 'k6_lite_distill', why: 'Small 3B model for short clips with synced speech/music/SFX.', frames: 121, steps: 10, guidance: 1, tip: 'Five seconds at 832x480/24fps to start; no guidance or negative prompt — describe scene, action, camera and sound together.' },
      { model: 'prism', why: 'Image-to-video with jointly generated audio, up to 2K.', frames: 81, steps: 20, guidance: 5, tip: 'Start image + motion/sound description; use the supplied defaults first. One pass — no end images or continuation.' }
    ]
  },
  {
    id: 'camera-move',
    label: 'Camera move from a photo',
    picks: [
      { model: 'fun_camera_1.3B', why: 'Small camera-control model; text or start image + preset path.', frames: 49, steps: 20, guidance: 5, tip: 'Begin with a modest move — hidden areas must be invented. No sliding windows.' },
      { model: 'uni3c', why: '14B camera-control guided by the start image geometry.', frames: 49, steps: 20, guidance: 5, tip: 'Requires a start image; keep the move modest for the same reason.' }
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
      { model: 'flux3_klein', why: 'FLUX 3 Klein 5.6B distilled — 16-step text-to-image plus up to 10 ordered references for edits.', frames: 0, steps: 16, guidance: 1, tip: 'Distilled flow-map checkpoint: no CFG in the default profile. The Base variant is 50 steps with CFG when a prompt needs more.' },
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
    id: 'vfx-alpha',
    label: 'Transparent video (alpha) / VFX',
    picks: [
      { model: 'ltx2_25_22B_alpha_gen', why: 'Extract a soft alpha matte from any video, then export the original RGB with transparency.', frames: 121, steps: 8, guidance: 1, tip: 'Configuration / Outputs / RGBA Video Output picks PNG frames in a ZIP (default) or ProRes 4444 MOV; the gallery video is the grayscale matte.' },
      { model: 'alpha2', why: 'Generate transparent video directly — hair, glow and smoke keep fine alpha.', frames: 81, steps: 25, guidance: 5, tip: 'Gallery shows a checkerboard preview over the alpha; the RGBA frames are the real output.' },
      { model: 'ltx2_25_22B_layout_to_render', why: 'Turn a viewport animation or playblast into a finished shot, with one appearance reference setting the look.', frames: 121, steps: 8, guidance: 1, tip: 'Two phases with tiling: phase 1 lays out the scene, phase 2 refines it in overlapping 50% tiles — the way to reach 4K.' }
    ]
  },
  {
    id: 'object-3d',
    label: '3D asset from a photo (v18)',
    picks: [
      { model: 'trellis2', why: 'GLB mesh with PBR textures (colour, metallic, roughness) from one image.', tip: 'One whole uncropped object on a plain background, three-quarter view, even light. Start with 1024 Cascade; hidden sides are inferred.' },
      { model: 'hunyuan3d_21', why: 'Shape + PBR texture, shape only, or texture your own mesh from a reference image.', tip: 'Texturing replaces the mesh UVs/materials. To start from text, generate an image first, then use it as the 3D reference.' },
      { model: 'worldmirror_2', why: 'Photos and/or a reference video to Gaussian splats or a point cloud with cameras.', tip: 'Overlapping views around a static scene; areas no view shows stay empty. Open results in the 3D gallery to inspect, then save a view as a video start image.' }
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
 * copies the full name. Verified 2026-09-21; the three vfx-alpha entries
 * re-verified against upstream v17.10 (ec9566a) on 2026-10-06; the four Veda
 * entries against upstream v18 (46005e5) on 2026-10-12. k6/prism/camera
 * picks intentionally unmapped until their defaults names are verified —
 * pickName falls back to the id.
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
  stable_audio3_small: 'Music Stable Audio 3 Small Music',
    ltx2_25_22B_alpha_gen: 'LTX-2 2.5 Alpha Gen 22B',
  ltx2_25_22B_layout_to_render: 'LTX-2 2.5 Layout to Render 22B',
  alpha2: 'Wan2.1 Alpha v2.0 14B',
  minimax_h3_fl2va_veda: 'MiniMax H3 FL2VA Veda Sparse 33B',
  minimax_h3_fl2va_pruned_veda: 'MiniMax H3 FL2VA Pruned Veda Sparse 20B',
  minimax_h3_ref2va_veda: 'MiniMax H3 Ref2VA Veda Sparse 33B',
  minimax_h3_ref2va_pruned_veda: 'MiniMax H3 Ref2VA Pruned Veda Sparse 20B',
  flux3_klein: 'Flux 3 Klein 5.6B',
  flux3_klein_base: 'Flux 3 Klein Base 5.6B',
  flux3_klein_nvfp4: 'Flux 3 Klein NVFP4 5.6B',
  flux3_klein_base_nvfp4: 'Flux 3 Klein Base NVFP4 5.6B',
  fish_drama3: 'TTS Fish Audio Drama 3',
  fish_drama3_int4: 'TTS Fish Audio Drama 3 INT4 Experts'
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
