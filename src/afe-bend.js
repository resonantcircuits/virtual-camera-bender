import { clamp, hashSigned, hashUnit, lerp } from "./utils.js";
import { RAW_MAX, BLACK_LEVEL } from "./raw-domain.js";

// Analog front end bend (physics rail, spec Phase 7 item 1). Before the ADC
// the pixel stream is a continuous voltage; this module attacks it the three
// ways real benders do, in one per-clock pass over the raw stream:
//
// - Oscillator injection (additive): the classic "audio into the video path"
//   bend. The oscillator frequency is parameterized in cycles per sensor row:
//   well below 1 the beat against row timing gives rolling horizontal hum
//   bands; tens of cycles per row give moire against the pixel clock. `skew`
//   adds per-row phase creep (band tilt), `wobble` seeded phase drift.
// - PGA/VREF wobble (multiplicative): the same oscillator modulating the
//   programmable gain / ADC reference instead — exposure that pumps.
// - CDS timing skew: correlated double sampling with the reset sample landing
//   on a stale pixel (out = v[n] - amount * (v[n - lag] - black)) — the frame
//   becomes a horizontal-derivative emboss with negative trails.
//
// All of it lands in Bayer raw before white balance and demosaic, so bands
// and trails pick up color exactly the way real analog interference does.

const OSC_MIN_CPR = 0.02; // cycles per row, log sweep floor (slow hum bands)
const OSC_MAX_CPR = 320; // and ceiling (pixel-clock moire)
const CDS_MAX_LAG = 48; // clocks

export function afeBendActive(config) {
  if (!config?.enabled) return false;
  return (
    clamp(config.inject ?? 0) > 0.004 ||
    clamp(config.gainMod ?? 0) > 0.004 ||
    clamp(config.cdsAmount ?? 0) > 0.004
  );
}

export function applyAfeBendRaw(raw, width, height, config, seed) {
  const wave = config.wave || "sine";
  const cyclesPerRow = Math.pow(
    10,
    lerp(Math.log10(OSC_MIN_CPR), Math.log10(OSC_MAX_CPR), clamp(config.freq ?? 0.35)),
  );
  const skew = clamp(config.skew ?? 0, -0.5, 0.5);
  const wobble = clamp(config.wobble ?? 0.1);
  // Perceptual tapers: half-scale injection already reads as violent.
  const injectAmp = Math.pow(clamp(config.inject ?? 0), 1.5) * 0.55 * (RAW_MAX - BLACK_LEVEL);
  const gainDepth = Math.pow(clamp(config.gainMod ?? 0), 1.3) * 0.9;
  const cdsAmount = clamp(config.cdsAmount ?? 0);
  const lag = Math.max(1, Math.round(Math.pow(10, Math.log10(CDS_MAX_LAG) * clamp(config.cdsSkew ?? 0.3))));

  const phasePerPixel = cyclesPerRow / width;
  let phase = hashUnit(1, 1, seed + 9101); // free-running oscillator, seeded start

  // CDS history ring holds the *uncorrupted* sensor stream `lag` clocks back.
  const hist = new Float32Array(lag);

  for (let y = 0; y < height; y += 1) {
    // Oscillator drift, re-walked per row so bands wander instead of locking.
    if (wobble > 0) phase += hashSigned(y, 3, seed + 9103) * wobble * 0.12;
    phase += skew;
    const row = y * width;
    for (let x = 0; x < width; x += 1) {
      const p = row + x;
      let v = raw[p];

      if (cdsAmount > 0) {
        const slot = p % lag;
        const stale = p >= lag ? hist[slot] : v;
        hist[slot] = v;
        v -= cdsAmount * (stale - BLACK_LEVEL);
      }

      let o;
      if (wave === "square") {
        o = phase - Math.floor(phase) < 0.5 ? 1 : -1;
      } else if (wave === "saw") {
        o = (phase - Math.floor(phase)) * 2 - 1;
      } else if (wave === "noise") {
        // Sample-and-hold noise: one random level per oscillator cycle.
        o = hashSigned(Math.floor(phase), 7, seed + 9107);
      } else {
        o = Math.sin(phase * Math.PI * 2);
      }

      if (gainDepth > 0) v = BLACK_LEVEL + (v - BLACK_LEVEL) * (1 + gainDepth * o);
      v += injectAmp * o;

      raw[p] = v <= 0 ? 0 : v >= RAW_MAX ? RAW_MAX : v + 0.5;
      phase += phasePerPixel;
    }
  }
}
