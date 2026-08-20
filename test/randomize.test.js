import test from "node:test";
import assert from "node:assert/strict";

import { BUILT_IN_PRESETS } from "../src/built-in-presets.js";
import { ADVANCED_DEFS, clonePreset, defaultPipeline } from "../src/presets.js";
import {
  MODULE_RANDOMIZERS,
  RANDOM_FAMILY_MODULES,
  randomizeModule,
  randomizePreset
} from "../src/randomize.js";

const FAMILY_KEYS = Object.keys(RANDOM_FAMILY_MODULES);
const DAMAGE_KEYS = Object.values(RANDOM_FAMILY_MODULES).flat();

test("the eight refinement families own every damage module exactly once", () => {
  assert.equal(FAMILY_KEYS.length, 8);
  assert.equal(new Set(DAMAGE_KEYS).size, DAMAGE_KEYS.length);

  const expected = ADVANCED_DEFS
    .filter(({ key, classicEdit }) => !classicEdit && key !== "osdOverlay")
    .map(({ key }) => key)
    .sort();
  assert.deepEqual([...DAMAGE_KEYS].sort(), expected);
  assert.deepEqual(
    Object.keys(MODULE_RANDOMIZERS).filter((key) => key !== "osdOverlay" && key !== "basicAdjustments").sort(),
    expected
  );
});

test("a seeded whole-camera roll is reproducible", () => {
  const first = randomizePreset(BUILT_IN_PRESETS[0], "global", "damaged", { randomSeed: 43110 });
  const second = randomizePreset(BUILT_IN_PRESETS[0], "global", "damaged", { randomSeed: 43110 });
  assert.equal(first.seed, second.seed);
  assert.equal(first.name, second.name);
  assert.deepEqual(first.pipeline, second.pipeline);
});

test("family rolls leave every module outside their ownership untouched", () => {
  for (const family of FAMILY_KEYS) {
    const base = clonePreset(BUILT_IN_PRESETS[0]);
    const next = randomizePreset(base, family, "shorted", { randomSeed: 8000 + FAMILY_KEYS.indexOf(family) });
    const owned = new Set(RANDOM_FAMILY_MODULES[family]);
    assert.equal(next.seed, base.seed, `${family} changed the render seed`);
    for (const key of Object.keys(base.pipeline)) {
      if (!owned.has(key)) assert.deepEqual(next.pipeline[key], base.pipeline[key], `${family} changed ${key}`);
    }
    assert.deepEqual(next.macros, base.macros, `${family} changed vestigial macros`);
  }
});

test("global composition density follows gentle, broken, and wrecked", () => {
  const totals = { bent: 0, damaged: 0, shorted: 0 };
  for (let seed = 1; seed <= 120; seed += 1) {
    for (const mode of Object.keys(totals)) {
      const preset = randomizePreset(BUILT_IN_PRESETS[0], "global", mode, { randomSeed: seed });
      const active = DAMAGE_KEYS.filter((key) => preset.pipeline[key].enabled).length;
      if (mode === "bent") assert.ok(active >= 1 && active <= 2, `gentle seed ${seed}: ${active}`);
      if (mode === "damaged") assert.ok(active >= 3 && active <= 8, `broken seed ${seed}: ${active}`);
      if (mode === "shorted") assert.ok(active >= 6, `wrecked seed ${seed}: ${active}`);
      totals[mode] += active;
    }
  }
  assert.ok(totals.bent < totals.damaged);
  assert.ok(totals.damaged < totals.shorted);
});

test("a whole-camera roll cannot leak inactive damage settings from the previous camera", () => {
  const base = clonePreset(BUILT_IN_PRESETS[0]);
  for (const key of DAMAGE_KEYS) {
    base.pipeline[key].enabled = true;
    base.pipeline[key].__stale = `old-${key}`;
  }
  const next = randomizePreset(base, "global", "bent", { randomSeed: 92014 });
  const defaults = defaultPipeline();
  for (const key of DAMAGE_KEYS) {
    if (!next.pipeline[key].enabled) assert.deepEqual(next.pipeline[key], { ...defaults[key], enabled: false });
  }
  assert.deepEqual(next.pipeline.basicAdjustments, base.pipeline.basicAdjustments);
});

test("every module's effective damage increases across intensity levels", () => {
  for (const key of DAMAGE_KEYS) {
    const averages = ["bent", "damaged", "shorted"].map((mode) => {
      let total = 0;
      for (let seed = 1; seed <= 80; seed += 1) {
        const preset = randomizeModule(BUILT_IN_PRESETS[0], key, mode, { randomSeed: seed * 7919 });
        total += moduleDamage(key, preset.pipeline[key]);
      }
      return total / 80;
    });
    assert.ok(averages[0] < averages[1], `${key}: gentle ${averages[0]} >= broken ${averages[1]}`);
    assert.ok(averages[1] < averages[2], `${key}: broken ${averages[1]} >= wrecked ${averages[2]}`);
  }
});

test("explore covers both restrained and destructive whole-camera builds", () => {
  const counts = [];
  for (let seed = 1; seed <= 160; seed += 1) {
    const preset = randomizePreset(BUILT_IN_PRESETS[0], "global", "explore", { randomSeed: seed });
    counts.push(DAMAGE_KEYS.filter((key) => preset.pipeline[key].enabled).length);
  }
  assert.ok(Math.min(...counts) <= 2);
  assert.ok(Math.max(...counts) >= 10);
});

function average(values) {
  return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : 0;
}

function moduleDamage(key, module) {
  if (key === "irCut") return module.strength;
  if (key === "ccdClock") return average([module.transferLoss, module.vSkip, module.hShear, module.bloom]);
  if (key === "afeBend") return average([module.inject, module.gainMod, module.cdsAmount]);
  if (key === "railSag") return average([module.sag, module.spikes, module.failures]);
  if (key === "busBend") return module.injectStrength;
  if (key === "masterClock") return Math.abs(module.detune) / 0.5 + module.shred;
  if (key === "addressBus") return Math.max(module.rows, module.cols, module.lowBit);
  if (key === "falseColor") return module.strength;
  if (key === "colorBend") {
    const active = [];
    if (module.hueRotate !== 0) active.push(module.hueStrength);
    if (module.channelMode !== "none") active.push(module.channelStrength);
    if (module.invert !== "none") active.push(module.invertStrength);
    if (module.solarize > 0) active.push(module.solarize);
    return average(active);
  }
  if (key === "gradientWash") return module.strength;
  if (key === "awbSeizure") return Math.max(module.wbSwing, module.aeSwing);
  if (key === "verticalSmear") return module.strength;
  if (key === "pixelSort") return module.strength;
  if (key === "exposureFault") return average([module.blackCrush, module.highlightClip, module.contourBands]);
  if (key === "contourRings") return module.strength;
  if (key === "edgeBurn") return module.strength;
  if (key === "sensorNoise") return module.amount;
  if (key === "ampGlow") return module.strength;
  if (key === "cheapCamera") return 1 - module.internalScale;
  if (key === "dctCrunch") return 1 - module.quality;
  if (key === "bayerFault") return module.strength;
  if (key === "syncFault") return Math.max(module.tearCount, module.wobbleAmount);
  if (key === "chromaShift") return module.amount;
  if (key === "memoryFault") {
    return average([module.interlace, module.blockShift, module.rowRepeat, module.scanlineDropout]);
  }
  if (key === "bufferGhost") return module.amount;
  throw new Error(`No damage score for ${key}`);
}
