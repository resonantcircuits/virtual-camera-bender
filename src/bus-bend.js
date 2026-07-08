import { clamp } from "./utils.js";
import { RAW_BITS } from "./raw-domain.js";

// Physics-based simulation of a logic-chip camera bend on the CCD ADC data
// bus (modeled on the PowerShot A520 "source/target selector" bend). Runs on
// the physics rail's raw buffer (see raw-domain.js): the 12-bit Bayer stream
// is replayed clock-by-clock in sensor readout order through the bend
// circuit. The characteristic looks (posterized rainbow contours, color
// negatives, chaotic scanlines) emerge from bit-level bus contention being
// amplified by white balance, demosaic, and gamma in the forward ISP.
//
// Circuit topology (per the bend schematic):
//   bus wires --[source DIP D11..D2]--> node S --1uF cap--> node F
//   node F: pot (variable R) to GND, feeds inverter input / flip-flop clock
//   node F or chip output --[function switches]--> node T
//   node T --[target selector D11..D4, GND]--> bus wires
// Selected pins within a bank short together (bus contention); pins selected
// in both banks merge S and T into one node, which is what lets the divide
// flip-flop clock itself off the pin it is corrupting.

// v2 extensions (spec Phase 8 item 3): `resistance` puts the benders' 20-100
// ohm protection resistor in the patch line — each pin's wire only partially
// follows the shared node, opening a continuum between clean and fully
// shorted buses (comparator noise speckles the in-between region).
// `commonBus` jumpers the source and target nodes into one shared bus, so
// composite multi-pin collisions resolve together (the Praktica DC42
// common-bus DIP array look) instead of pairwise through the cap/chip.

// Bank wiring on the real bend: source DIP reaches D11..D2, target selector
// reaches D11..D4 plus a GND position.
export const SOURCE_BITS_MASK = 0b111111111100;
export const TARGET_BITS_MASK = 0b111111110000;

function maskBits(mask) {
  const bits = [];
  for (let i = RAW_BITS - 1; i >= 0; i -= 1) {
    if (mask & (1 << i)) bits.push(i);
  }
  return bits;
}

// True when the switch configuration creates contention somewhere:
// source-bank shorts, target-bank shorts (incl. GND pull-down), or a full
// source -> target injection path.
export function busBendActive(config) {
  if (!config?.enabled) return false;
  const srcN = maskBits((config.sourceMask | 0) & SOURCE_BITS_MASK).length;
  const tgtN = maskBits((config.targetMask | 0) & TARGET_BITS_MASK).length;
  const inject = srcN >= 1 && tgtN >= 1;
  return inject || srcN >= 2 || tgtN >= 2 || (tgtN >= 1 && !!config.targetGnd);
}

export function applyBusBendRaw(raw, width, height, config, seed, liveSeed = seed) {
  const sourceMask = (config.sourceMask | 0) & SOURCE_BITS_MASK;
  const targetMask = (config.targetMask | 0) & TARGET_BITS_MASK;
  const targetGnd = !!config.targetGnd;
  const srcBits = maskBits(sourceMask);
  const tgtBits = maskBits(targetMask);
  const srcN = srcBits.length;
  const tgtN = tgtBits.length;
  const inject = srcN >= 1 && tgtN >= 1;
  const tgtContended = tgtN >= 2 || (tgtN >= 1 && targetGnd) || inject;

  const size = width * height;
  const fn = config.fn || "bypass";
  const pot = clamp(config.pot ?? 0.5);
  const strength = clamp(config.injectStrength ?? 0.55);
  const jitter = clamp(config.jitter ?? 0.08) * 0.5;
  const resistance = clamp(config.resistance ?? 0);
  // Patch-line coupling: 0 ohm follows the node exactly (v1 hard short),
  // full resistance leaves the pin mostly driving its own wire.
  const k = 1 / (1 + 2.2 * resistance * resistance);

  // --- analog constants ---
  // potR is the pot position as a fraction of full resistance, swept on a
  // fast taper so the low end reaches a near-short to ground. The RC time
  // constant is normalized against a full-resolution sensor row (3072 px) so
  // streak lengths stay comparable across render resolutions.
  const potR = 0.004 + Math.pow(pot, 2.2);
  const tau = potR * 20000 * (width / 3072);
  const alpha = tau / (tau + 1);
  // DC load the pot puts on whatever node it hangs off (bypass mode only):
  // ADC drive impedance vs pot resistance. Low pot = hard pull toward GND.
  const wPot = 0.09 / potR;
  // Edge amplitude surviving the cap/pot divider.
  const capAmp = strength * 2.6 * (potR / (potR + 0.09));
  const wChip = 0.6 + strength * 4;
  const wGnd = targetGnd ? 3 : 0;

  const merged = inject && (!!config.commonBus || (sourceMask & targetMask) !== 0);
  const unionBits = merged ? maskBits(sourceMask | targetMask) : null;
  const unionN = merged ? unionBits.length : 0;

  // Comparator/threshold noise: analog, so a plain per-frame stream keyed by
  // liveSeed (shimmer per video frame) rather than coordinate-stable hashing.
  let rngState = ((liveSeed | 0) * 2654435761 + 0x9e3779b9) >>> 0;
  const rnd = () => {
    rngState = (rngState * 1664525 + 1013904223) >>> 0;
    return rngState / 4294967296;
  };

  // --- per-clock circuit state ---
  const thresholdHi = 0.26;
  const thresholdLo = 0.12;
  let hp = 0; // high-pass filter node voltage (bipolar, rests at 0)
  let prevIn = -1;
  let logicLevel = 0; // Schmitt-conditioned chip input
  let ff = seed & 1; // divide-by-two flip-flop state
  let out = fn === "invert" ? 1 : fn === "divide" ? ff : 0;
  let u = 0; // filtered analog signal presented to the bypass path

  for (let p = 0; p < size; p += 1) {
    let v = raw[p];
    let nodeIn = 0;

    if (merged) {
      // Overlapping pins short S and T into one node: every selected pin,
      // the GND switch, and the chip/cap injection all fight there.
      let sum = 0;
      for (let i = 0; i < unionN; i += 1) sum += (v >>> unionBits[i]) & 1;
      let node;
      if (fn === "bypass") {
        node = sum / (unionN + wPot + wGnd) + u * capAmp;
      } else {
        node = (sum + out * wChip) / (unionN + wChip + wGnd);
      }
      for (let i = 0; i < unionN; i += 1) {
        const bit = unionBits[i];
        const wire = ((v >>> bit) & 1) * (1 - k) + node * k;
        if (wire + (rnd() - 0.5) * jitter > 0.5) v |= 1 << bit;
        else v &= ~(1 << bit);
      }
      nodeIn = node;
    } else {
      let s = 0;
      for (let i = 0; i < srcN; i += 1) s += (v >>> srcBits[i]) & 1;
      const nodeS = srcN > 0 ? s / srcN : 0;
      if (srcN >= 2) {
        // Intra-bank short: contended source pins pull toward the mix, as
        // hard as the patch-line resistance lets them.
        for (let i = 0; i < srcN; i += 1) {
          const bit = srcBits[i];
          const wire = ((v >>> bit) & 1) * (1 - k) + nodeS * k;
          if (wire + (rnd() - 0.5) * jitter > 0.5) v |= 1 << bit;
          else v &= ~(1 << bit);
        }
      }
      if (tgtContended && tgtN > 0) {
        let t = 0;
        for (let i = 0; i < tgtN; i += 1) t += (v >>> tgtBits[i]) & 1;
        let nodeT;
        if (!inject) {
          nodeT = t / (tgtN + wGnd);
        } else if (fn === "bypass") {
          // Bypass connects targets straight to the filter node: pot DC load
          // plus edge energy coupled through the cap.
          nodeT = t / (tgtN + wPot + wGnd) + u * capAmp;
        } else {
          nodeT = (t + out * wChip) / (tgtN + wChip + wGnd);
        }
        for (let i = 0; i < tgtN; i += 1) {
          const bit = tgtBits[i];
          const wire = ((v >>> bit) & 1) * (1 - k) + nodeT * k;
          if (wire + (rnd() - 0.5) * jitter > 0.5) v |= 1 << bit;
          else v &= ~(1 << bit);
        }
      }
      nodeIn = nodeS;
    }
    raw[p] = v;

    // Analog state advances one propagation delay behind the injection,
    // sampling the post-contention wire (this is the DIV feedback loop).
    if (inject) {
      if (prevIn < 0) prevIn = nodeIn;
      hp = alpha * (hp + nodeIn - prevIn);
      prevIn = nodeIn;
      u = hp;
      if (fn !== "bypass") {
        if (logicLevel === 0 && u > thresholdHi + (rnd() - 0.5) * jitter * 0.4) {
          logicLevel = 1;
          ff ^= 1;
        } else if (logicLevel === 1 && u < thresholdLo) {
          logicLevel = 0;
        }
        out = fn === "invert" ? 1 - logicLevel : ff;
      }
    }
  }
}
