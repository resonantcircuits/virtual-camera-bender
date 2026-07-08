import { clamp, hashUnit, hashSigned } from "./utils.js";
import { RAW_MAX, BLACK_LEVEL } from "./raw-domain.js";

// Supply-rail bend (physics rail, spec Phase 8 item 1). Brownout is the
// physical cause behind the glitch statistics screen-space approximations
// hand-roll as coin flips: the 3.3V rail is one slow stochastic envelope
// V(y), sampled once per readout row, and everything follows from where it
// sits.
//
// - Above regulation nothing shows: the regulator holds.
// - Below regulation the ADC reference droops, so codes inflate (rows bloom
//   bright and clip) and charge injection lifts the black pedestal.
// - In the failure band, rows fail outright with probability shaped by the
//   deficit: ADC latch-up (the converter's interleaved sample stages stick
//   on a code pair -> solid color bands via WB and demosaic), comparator
//   collapse (full-range static), or complete dropout (black). Because the
//   envelope has memory, failures cluster into runs; no per-row dice decide
//   the composition.
// - Load spikes plunge the rail for a few rows and recover exponentially,
//   so even a healthy rail throws isolated failure bursts.

const V_REG = 0.92; // regulator dead zone: above this the rail holds clean
const SPIKE_DECAY = 0.86; // per-row recovery from a load transient

const FAIL_LATCH = 0;
const FAIL_DROP = 1;
const FAIL_STATIC = 2;

export function railSagActive(config) {
  if (!config?.enabled) return false;
  return clamp(config.sag ?? 0) > 0.004 || clamp(config.spikes ?? 0) > 0.004;
}

export function applyRailSagRaw(raw, width, height, config, seed, liveSeed = seed) {
  const sag = clamp(config.sag ?? 0);
  const flicker = clamp(config.flicker ?? 0);
  const spikes = clamp(config.spikes ?? 0);
  const failures = clamp(config.failures ?? 0);

  // Rail envelope: a random walk pulled toward the loaded operating point,
  // driven by low-passed noise so sags are sustained dips (broad breathing
  // bands, clustered failures), not per-row chatter. Rates are normalized to
  // a 960-row frame so the band scale is resolution-independent (same
  // reasoning as busBend's RC normalization).
  const rowRate = Math.min(2.5, 960 / height);
  const level = 1 - 0.38 * sag;
  const pull = (0.05 + 0.25 * flicker) * rowRate;
  const smoothK = Math.min(0.85, (0.04 + 0.45 * flicker * flicker) * rowRate);
  // Filtered noise loses amplitude; compensate so the sag depth the knob
  // dials stays put while flicker only reshapes the spectrum.
  const vol = ((0.02 + 0.1 * flicker) * (0.35 + 0.65 * sag) * rowRate) / Math.sqrt(smoothK);
  const spikeP = spikes > 0 ? (0.0015 + spikes * spikes * 0.03) * rowRate : 0;
  const spikeDecay = Math.pow(SPIKE_DECAY, rowRate);
  // Per-frame supply shimmer so video breathes; stills are seed-stable.
  const jitterAmp = 0.02 * (0.3 + flicker);
  // Failure threshold rises with the failures knob: shallower sags fail.
  const vFail = 0.42 + 0.3 * failures;

  let v = level + hashSigned(0, 11, seed) * 0.03;
  let drive = 0;
  let spikeLevel = 0;
  let failRowsLeft = 0;
  let failType = FAIL_DROP;
  let latchA = 0;
  let latchB = 0;

  // Static rows are broadband noise at the comparator: per-frame stream, not
  // coordinate-stable hashing (same reasoning as busBend comparator jitter).
  let rngState = ((liveSeed | 0) * 2654435761 + 0x9e3779b9) >>> 0;
  const rnd = () => {
    rngState = (rngState * 1664525 + 1013904223) >>> 0;
    return rngState / 4294967296;
  };

  const range = RAW_MAX - BLACK_LEVEL;

  for (let y = 0; y < height; y += 1) {
    drive += smoothK * (hashSigned(y, 1, seed) - drive);
    v += pull * (level - v) + vol * drive;
    v = clamp(v, 0.05, 1.05);
    if (spikeP > 0 && hashUnit(y, 2, seed) < spikeP) {
      const depth = (0.18 + 0.45 * hashUnit(y, 3, seed)) * (0.4 + 0.6 * Math.max(sag, spikes));
      spikeLevel = Math.max(spikeLevel, depth);
    }
    spikeLevel *= spikeDecay;
    const vEff = v - spikeLevel + jitterAmp * hashSigned(y, 4, liveSeed);

    // Failure gate: entry probability ramps with how far the rail sits below
    // the failure threshold. Once triggered, a failure is self-sustaining
    // (real latch-up holds until the supply resets), so the event runs for a
    // drawn duration in normalized rows — thickness is resolution-independent
    // and the entry rate scales so event count per frame stays put.
    if (failures > 0 && failRowsLeft === 0) {
      const pFail = clamp((vFail - vEff) * (1.5 + 6 * failures));
      if (pFail > 0 && hashUnit(y, 5, seed) < clamp(pFail * rowRate)) {
        failRowsLeft = Math.max(
          1,
          Math.round((0.6 + hashUnit(y, 9, seed) * (2.5 + 13 * failures)) / rowRate),
        );
        const t = hashUnit(y, 6, seed);
        failType = t < 0.45 ? FAIL_LATCH : t < 0.75 ? FAIL_DROP : FAIL_STATIC;
        // Interleaved AFE sample stages latch as a pair, so stuck rows carry
        // two alternating codes -> real color once WB and demosaic develop it.
        latchA = (hashUnit(y, 7, seed) * (RAW_MAX + 1)) | 0;
        latchB = (hashUnit(y, 8, seed) * (RAW_MAX + 1)) | 0;
      }
    }

    const row = y * width;
    if (failRowsLeft > 0) {
      failRowsLeft -= 1;
      if (failType === FAIL_LATCH) {
        for (let x = 0; x < width; x += 1) raw[row + x] = x & 1 ? latchB : latchA;
      } else if (failType === FAIL_DROP) {
        raw.fill(0, row, row + width);
      } else {
        for (let x = 0; x < width; x += 1) raw[row + x] = (rnd() * (RAW_MAX + 1)) | 0;
      }
      continue;
    }

    // Regulated rows pass untouched; sagging rows bloom. VREF droop inflates
    // the codes (ADC measures signal against a sagging reference) and charge
    // injection lifts the pedestal, so brownout bands brighten before they die.
    const deficit = V_REG - vEff;
    if (deficit <= 0.0005) continue;
    const gain = 1 + deficit * 2.2;
    const pedestal = deficit * 0.16 * range;
    for (let x = 0; x < width; x += 1) {
      const p = row + x;
      const out = BLACK_LEVEL + (raw[p] - BLACK_LEVEL) * gain + pedestal;
      raw[p] = out <= 0 ? 0 : out >= RAW_MAX ? RAW_MAX : out + 0.5;
    }
  }
}
