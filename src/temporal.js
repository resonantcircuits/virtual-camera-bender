import { ADVANCED_DEFS, clonePreset } from "./presets.js";
import { clamp, fbmNoise, getAtPath, hashUnit, setAtPath } from "./utils.js";

// Temporal scheduling for video rendering. Everything here is pure and
// browser-free: the same functions drive the web preview (contact sheet and
// per-frame working image) and the CLI's render-video loop, so a chosen
// frame previews exactly what the CLI will produce for it.
//
// Two seed tiers per frame:
// - the structural seed follows temporal.mode (locked / hold / flicker) and
//   drives everything hash-seeded in the engine: tears, block corruption,
//   dead columns, ghost blocks, sort columns;
// - the live seed changes every frame regardless, and drives only the
//   noise-texture draws (sensor grain, speckle, hot pixels, amp-glow grain)
//   so a locked fault still shimmers like a running sensor instead of
//   looking like a frozen overlay.

const STRUCTURAL_STRIDE = 7919;
const LIVE_STRIDE = 104729;

// In hold mode segment lengths vary ±40% around holdFrames (seeded), so the
// glitch stutters irregularly instead of reconfiguring on a metronome.
function holdSegment(frameIndex, holdFrames, seed) {
  const base = Math.max(1, Math.round(holdFrames) || 12);
  let segment = 0;
  let start = 0;
  for (;;) {
    const length = Math.max(1, Math.round(base * (0.6 + 0.8 * hashUnit(segment, 17, seed))));
    if (frameIndex < start + length) return segment;
    start += length;
    segment += 1;
  }
}

export function temporalSeeds(preset, frameIndex) {
  const base = Number.isFinite(preset.seed) ? preset.seed : 1;
  if (!Number.isFinite(frameIndex)) return { seed: base, liveSeed: base };
  const temporal = preset.temporal || {};
  let structural = base;
  if (temporal.mode === "flicker") {
    structural = base + STRUCTURAL_STRIDE * frameIndex;
  } else if (temporal.mode === "hold") {
    structural = base + STRUCTURAL_STRIDE * holdSegment(frameIndex, temporal.holdFrames, base);
  }
  return { seed: structural, liveSeed: base + LIVE_STRIDE * (frameIndex + 1) };
}

// Params eligible for drift: every continuous range control in the advanced
// panel. Integer-stepped params (bitDepth, phaseError, generations, …) are
// excluded — they would pop between discrete states instead of wandering.
const DRIFTABLE = ADVANCED_DEFS.flatMap((group) =>
  group.controls
    .filter(([, , type, , , step]) => type === "range" && step < 1)
    .map(([path, , , min, max]) => ({
      path,
      moduleKey: group.key,
      min,
      max,
      span: max - min
    }))
);

// Each param wanders around its preset value on its own smooth noise track
// (phase from the param path, time from the frame index). driftAmount 1
// allows ±30% of the control's full range; driftAmount 0 is a no-op, so
// stills and drift-free presets render exactly as before. Mutates in place.
export function applyTemporalDrift(preset, frameIndex) {
  const temporal = preset.temporal || {};
  const amount = clamp(temporal.driftAmount ?? 0);
  if (!(amount > 0.001) || !Number.isFinite(frameIndex)) return preset;

  const speed = clamp(temporal.driftSpeed ?? 0.3);
  const time = frameIndex * (0.003 + speed * speed * 0.06);
  const seed = (Number.isFinite(preset.seed) ? preset.seed : 1) + 6151;

  DRIFTABLE.forEach((param, index) => {
    if (!preset.pipeline?.[param.moduleKey]?.enabled) return;
    const value = getAtPath(preset, param.path);
    if (typeof value !== "number") return;
    const phase = index * 3.7 + hashUnit(index, 29, seed) * 90;
    const offset = (fbmNoise(time, phase, seed) - 0.47) / 0.47;
    setAtPath(
      preset,
      param.path,
      clamp(value + offset * amount * param.span * 0.3, param.min, param.max)
    );
  });
  return preset;
}

// One-stop per-frame preparation: clone, drift (from the base seed, so
// parameter tracks stay continuous across hold-mode seed jumps), then swap
// in the structural seed. Returns the input untouched for stills.
export function prepareTemporalFrame(preset, frameIndex) {
  if (!Number.isFinite(frameIndex)) return { preset, liveSeed: undefined };
  const seeds = temporalSeeds(preset, frameIndex);
  const framePreset = clonePreset(preset);
  applyTemporalDrift(framePreset, frameIndex);
  framePreset.seed = seeds.seed;
  return { preset: framePreset, liveSeed: seeds.liveSeed };
}
