import { clamp, hashSigned, hashUnit } from "./utils.js";

// Master-clock reclock bend (physics rail, spec Phase 8 item 2): the LTC1799
// mod. Replacing the master oscillator makes the DSP capture the sensor's
// pixel stream at the wrong rate, so this runs last on the rail — it
// re-frames everything upstream. The model is one continuous sampling
// position walking the source stream at `ratio` source pixels per output
// pixel:
//
// - ratio != 1 stretches or compresses content along the scan, and the
//   leftover line phase accumulates row over row: skew, then full vertical
//   roll once the error spans whole lines.
// - The DSP's H-sync PLL only sees *line* phase: each row start, the error
//   modulo one line is measured, and if it falls inside the capture range
//   the PLL pulls it in. Vertical roll survives lock (rolled picture, stable
//   lines); an error drifting in and out of capture snaps and rolls in
//   alternating bands — tears emerge, none are scripted.
// - `drift` wobbles the ratio with slow seeded noise: wavy tape-ish skew.
//
// The DSP's pixel counters normally keep the Bayer quad aligned even while
// the framing slips, so sampling offsets are snapped to CFA parity and the
// geometry warps without color damage. `shred` is the fraction of rows whose
// sample clock slips at pixel level instead: those rows capture the mosaic
// out of phase and develop into colored line static — the documented
// "heavy color-phase drift" of real reclock bends, dosed by a knob.
// Ratio error and PLL dynamics are normalized to a 960-row frame so the
// roll and skew read the same across render resolutions.

export function masterClockActive(config) {
  if (!config?.enabled) return false;
  return Math.abs(clamp(config.detune ?? 0, -0.5, 0.5)) > 0.004 || clamp(config.drift ?? 0) > 0.004;
}

export function applyMasterClockRaw(raw, width, height, config, seed) {
  const detune = clamp(config.detune ?? 0, -0.5, 0.5);
  const drift = clamp(config.drift ?? 0);
  const hLock = clamp(config.hLock ?? 0.5);
  const shred = clamp(config.shred ?? 0.2);
  const size = width * height;
  const rowRate = Math.min(2.5, 960 / height);

  // Perceptual detune taper: the first few percent are VHS-subtle, mid-knob
  // is a structured roll, and only the ends of the range collapse sync
  // entirely (12% pixel-rate error — well past any PLL).
  const ratioErr = Math.sign(detune) * Math.pow(Math.abs(detune) * 2, 2.2) * 0.12 * rowRate;
  const driftAmp = drift * drift * 0.04 * rowRate;
  const driftK = Math.min(0.85, 0.05 * rowRate);
  const capture = Math.pow(hLock, 1.5) * 0.35 * width;
  const halfLine = width / 2;
  const shredP = Math.pow(shred, 1.2);

  const src = raw.slice();
  let pos = 0;
  let drive = 0;
  for (let y = 0; y < height; y += 1) {
    drive += driftK * (hashSigned(y, 41, seed) - drive);
    const ratio = 1 + ratioErr + driftAmp * drive;

    // H-PLL: line-phase error, wrapped to [-halfLine, halfLine).
    let err = (pos - y * width) % width;
    if (err >= halfLine) err -= width;
    else if (err < -halfLine) err += width;
    if (err !== 0 && Math.abs(err) <= capture) pos -= err * 0.6;

    const snap = shredP === 0 || hashUnit(y, 42, seed) >= shredP;
    const row = y * width;
    for (let x = 0; x < width; x += 1) {
      let s = pos | 0;
      if (s >= size) s -= size;
      else if (s < 0) s += size;
      if (snap) {
        // Force the sample onto the output pixel's CFA parity, horizontally
        // and vertically: framing slips, the mosaic stays aligned.
        let sy = (s / width) | 0;
        let sx = s - sy * width;
        sx = (sx & ~1) | (x & 1);
        sy = (sy & ~1) | (y & 1);
        if (sy >= height) sy -= 2;
        s = sy * width + sx;
      }
      raw[row + x] = src[s];
      pos += ratio;
    }
  }
}
