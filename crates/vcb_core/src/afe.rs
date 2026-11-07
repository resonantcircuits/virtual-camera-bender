//! Analog front end: charge samples converted to the voltage at the ADC input.
//!
//! The model follows an AD9949-class CCD signal processor
//! (`docs/research/a520-technical.md`, section 5). It runs in continuous time on the
//! pixel clock, `oversample` sub-steps per pixel period, in `f64`, and carries its
//! analog state ([`State`]) from line to line and from frame to frame.
//!
//! # Signal chain
//!
//! 1. **CCD output and CDS.** Correlated double sampling turns each charge packet into
//!    a level of `conv_gain_uv_per_e` microvolts per electron above the reset level,
//!    held for the pixel period: the level the packet adds to the CCD's floating
//!    diffusion, which the reset gate empties before every packet (see "Reset gate"
//!    for missed and weak pulses). With a clamp-timing skew (`[bend.afe] cds_lag_px`,
//!    `cds_mix`) the reset sample is taken `cds_lag_px` pixel periods late, so the
//!    fraction `cds_mix` of the level that many periods earlier is subtracted: every
//!    edge gets a negative echo further along the line. Before the line's first
//!    sample lies blanking, which contributes no signal.
//! 2. **Read noise**, added at the input. A white Gaussian part of `read_noise_e`
//!    electrons rms per sample, and a flicker part: a first-order low-pass Gaussian
//!    process with corner `noise_1f_corner_hz` whose spectral density at the corner
//!    equals that of the white floor, so the noise is streaky along lines. Both come
//!    from one [`Pcg32`] stream seeded with the stage seed and the frame index.
//! 3. **Injection** ([`Inject`]). An oscillator adds its waveform at the input node. It
//!    runs in real time over active samples and blanking; `phase_skew` adds a fixed
//!    phase step per line, which tilts the bands.
//! 4. **Programmable gain** of `pga_gain_db`, the VGA register's setting, times the
//!    pixel gain of the sample's mosaic phase (the `PxGA` registers, 0 dB on the A520),
//!    optionally modulated multiplicatively by a second oscillator ([`GainMod`]) with
//!    its own phase. See "Serial interface".
//! 5. **Bandwidth and slew.** The amplified sum drives a first-order low-pass of cutoff
//!    `bandwidth_hz` whose output moves at most `slew_v_per_us`. Each sub-step holds
//!    its input constant, so the exponential response is exact within it; the slew
//!    limit then clamps the step. The ADC samples the node at the end of each pixel
//!    period.
//! 6. **Pedestal and reference.** The black-level clamp adds the pedestal, the clamp
//!    register's level, so zero charge lands on the nominal black code. The output is
//!    referred to the nominal ADC reference [`Params::vref_v`]: the voltage the
//!    converter sees, measured against the reference the supply delivers
//!    ([`State::rail_v`]), as the code that voltage would give at the nominal reference.
//!
//! Without noise, bends, bandwidth limits or a sagging supply the chain reduces to
//! `volts = pedestal_v + electrons * conv_gain * 10^(pga_gain_db / 20)`.
//!
//! # Supply
//!
//! The chip runs from the supply rail of [`power`](crate::power), read per sample
//! from the [`Rail`] at the end of each pixel period, linear within each line. With
//! `r = rail / nominal`:
//!
//! - **Ratiometric reference and pedestal.** The ADC's reference and the clamp's
//!   pedestal both follow the rail, so the converter sees `(pedestal_v r + node) /
//!   (vref_v r)` of full scale and the output is `pedestal_v + node / r`: codes above
//!   black inflate by `1 / r` as the rail sags (rows bloom bright), and black stays
//!   put. An AD9949 rejects supply variations by 50 dB inside its 2.7 to 3.6 V range
//!   (`docs/research/a520-technical.md`, section 5); the ratiometric reference is the
//!   plan's model of a reference that has lost its headroom (PROVISIONAL, plan
//!   section 3.9).
//! - **Undervoltage lockout.** Below [`Params::uvlo_v`] the chip stops converting:
//!   its output drivers tristate and the bus lines' capacitance holds the last word,
//!   which the processor reads again and again. The output holds the last voltage,
//!   rounded to the centre of its code ([`Params::code_v`]), so the converter, whose
//!   dither stays within a third of a code, repeats that word: every sample of a
//!   locked-out stretch, optical black included, carries it. The analog chain keeps
//!   its state meanwhile and resumes from it.
//! - **Restart.** When the rail returns above the lockout the black clamp
//!   re-establishes the pedestal: it starts from the level the lockout left it at,
//!   `pedestal_v * uvlo_v / nominal`, and relaxes towards `pedestal_v r` with the
//!   time constant [`Params::restart_s`] (`afe_restart_lines` native lines), per
//!   pixel period and across blanking. Black dips while the rail recovers faster than
//!   the clamp; the image processor's own optical-black clamp follows the dip with
//!   its four-line lag, so a lockout leaves a few lines of shifted shadows behind it.
//!
//! A rail at the nominal voltage on a whole line takes the plain path, `r = 1`, which
//! gives the ideal supply's bytes.
//!
//! # Reset gate
//!
//! The CCD's output stage turns charge into voltage on its floating diffusion. Before
//! each packet the reset gate empties the diffusion into the reset drain; correlated
//! double sampling reads the diffusion's level after the reset and again after the
//! packet has arrived, and the difference is the packet. `[bend.reset]` breaks the
//! gate's pulse, a wire on the sensor ribbon. The diffusion's charge `q`, in
//! electrons, is part of [`State`], and in every pixel period
//!
//! > the reset pulse, if it arrives, clears `reset_frac` of `q`; the reset sample reads
//! > `q_reset = q`; the packet `e` joins, `q += e`; and the CDS reads
//! > `min(q, q_max) - min(q_reset, q_max)` electrons,
//!
//! where `q_max = fd_swing_v / conv_gain` is the output stage's swing in electrons
//! (`[afe] fd_swing_v`, PROVISIONAL: the CCD's source follower runs out before the
//! AFE does, so the swing is its saturation signal of 460 mV,
//! `docs/research/a520-technical.md`, section 3, plus a margin, 0.6 V on the A520;
//! the AD9949's CDS input of 1.0 V p-p before saturation, section 5, C, bounds it
//! from above). Charge on the diffusion beyond the
//! swing no longer moves the output, so a packet that lands there is lost and reads
//! the pedestal: black. Below it the difference is exactly the packet, so the CDS
//! cancels a missed or weak reset until the charge left on the diffusion uses up the
//! swing. In blanking no packets arrive and the gate's pulses act on nothing: the
//! diffusion keeps its charge until the reset at the next line's first sample, and
//! from the end of one frame to the start of the next. The charge is unbounded; the
//! spill of an over-full diffusion over the reset gate's barrier is not modelled,
//! which matters only for how fast a weak reset drains it after a run of missed pulses.
//!
//! **Missed pulses** (`drop_hz`, `hold_px`, a flaky contact). A run of missed pulses
//! starts with probability `drop_hz * pixel_period_s` in each pixel period while none
//! runs, blanking included (a Poisson process in camera time), and lasts a geometric
//! number of periods with a mean of `hold_px` periods of the camera's own clock (see
//! "Time scaling"). The runs come from a [`Pcg32`] seeded with the bend's own stage seed
//! (salt `"reset"`) and the frame index, in readout order, drawn as the lengths of the
//! alternating stretches of sound and missed pulses: the exact equivalent of one trial
//! per period. Whether a run is in progress at the end of a frame is part of the
//! [`State`]; the next frame draws what is left of it afresh from its own stream,
//! which the geometric law's lack of memory makes exact. Without drops a run in
//! progress ends at once, the contact being sound. During a run the diffusion holds
//! the packet before it and adds every later one: over a bright run of packets `e`
//! it reads normally while the sum stays within the swing, at most `q_max / e - 1`
//! packets, then one in part, then black until the run ends and the reset returns. A
//! dark run whose sum stays below the swing reads unchanged. A run that spans blanking
//! carries the charge into the next line, one that spans VD into the next frame.
//!
//! **Weak pulses** (`reset_frac = r` below 1). Each reset leaves `(1 - r) q` behind.
//! On a uniform run of packets `e` arriving at an empty diffusion the charge after
//! `n` packets is
//!
//! > `q_n = (e / r) (1 - (1 - r)^n)`,
//!
//! and the CDS reads `e` while `q_n <= q_max`, then `q_max - (1 - r) q_(n-1)` (floored
//! at zero), falling within a few pixels towards `max(0, q_max - (1 - r) e / r)`. A
//! run with `e <= r q_max` never reaches the swing and reads unchanged; brighter runs
//! dim, and from `e >= r q_max / (1 - r)` on they end in black, brighter light reading
//! darker. The dark optical-black samples at the start of each line drain the
//! diffusion by `(1 - r)^ob_cols` before the active ones.
//!
//! Optical-black samples pass through the same diffusion, so the image processor's
//! black clamp sees what the bend does to them. Read noise arises behind the
//! diffusion, in the output amplifier and the AFE, and is added after the CDS. With
//! `drop_hz = 0` and `reset_frac = 1` the step is skipped and every packet reads as
//! itself, which gives the bytes of a camera without the section.
//!
//! # Serial interface
//!
//! The processor programs the chip over its three-wire serial interface (SL, SCK,
//! SDATA; AD9949 class, `docs/research/a520-technical.md`, section 5). Each write is
//! an [`ADDRESS_BITS`]-bit register address (C) followed by a data field of
//! `[afe] serial_data_bits` (PROVISIONAL 24), a register narrower than the field
//! taking its low bits and the rest padding. The chip latches the data and applies it
//! at the next VD (C for the family, whose registers are VD-updated unless held;
//! PROVISIONAL for the A520's use of it). The modelled registers, their addresses
//! PROVISIONAL:
//!
//! | Address | Register | Bits | Setting of code `c` | Nominal content |
//! |---|---|---|---|---|
//! | 0x00 | control | 2 | bit 0 power-down, bit 1 output disable | 0 |
//! | 0x01 | VGA gain | `vga_bits` (C: 10) | `vga_min_db + vga_db_per_code * c` dB (C: 6 dB, 0.0351 dB) | `pga_gain_db` |
//! | 0x02 to 0x05 | `PxGA` gain R, Gr, Gb, B | `pxga_bits` (PROVISIONAL 6) | `pxga_max_db * c / (2^pxga_bits - 1)` dB (C: 18 dB) | `pxga_gain_db` |
//! | 0x06 | clamp level | `clamp_bits` (C: 8) | pedestal `c * code_v`, codes of the converter's full width | `pedestal_v` |
//!
//! The camera's values become codes by the nearest step, `round((pga_gain_db -
//! vga_min_db) / vga_db_per_code)` and so on, and [`Params::resolve`] refuses a camera
//! whose value does not fit its register; the A520's 6.2 dB is VGA code 6, its
//! 0.0303 V pedestal clamp code 124, its `PxGA` gains code 0. A register holding its
//! nominal code applies the camera's exact value, not the code's quantised one, so a
//! clean write leaves the camera as it is.
//!
//! **Writes.** In every vertical blanking, before the frame's first line, the
//! processor rewrites the whole set in address order, each register with its nominal
//! code (PROVISIONAL: a firmware that wrote only changes would let a corrupted register
//! stick until the next change). `[bend.serial] bit_error` is a noisy contact on
//! SDATA: each bit of each write, the address bits A0 to A7 and then the data bits D0
//! upwards, padding included, reads wrong at the chip with probability `bit_error`,
//! drawn in that order from a [`Pcg32`] seeded with the bend's own stage seed (salt
//! `"serial"`) and the frame index. A write whose address survives lands its data,
//! flips and all, truncated to the register's width; a write whose address flips
//! lands on the register at the flipped address if it is one of the seven, truncated
//! to that register's width, and is lost otherwise. A register no write reaches keeps
//! its content, the last frame's, corrupted or not, until a clean write. The registers
//! are [`State`], carried from frame to frame and kept on reconfiguration, nominal at
//! power-on. With `bit_error = 0`, or without the section, every write is clean and
//! the registers hold the camera's values. At `bit_error = 1` every bit flips, so every
//! address turns into one at 0xF9 or above and every write is lost: the registers keep
//! what they held.
//!
//! **Effect.** At VD the registers take effect for the frame. The VGA gain replaces
//! `pga_gain_db`. Each `PxGA` gain multiplies the samples of its mosaic phase in the
//! gain step: the AFE learns each sample's colour from the readout line's image row
//! and the sample's column in the line, which on the RGGB mosaic is a red row (R, Gr)
//! on even image rows and a blue row (Gb, B) on odd ones; the movie readout keeps row
//! pairs and so the phase, and the optical-black columns take the phase of their
//! position, continuing the active columns' alternation to the left. At 0 dB the
//! factors are exactly 1 and the step is skipped. Over blanking, where no sample has
//! a colour, the node relaxes with the VGA gain alone. The clamp level sets the pedestal.
//! A set power-down or output-disable bit tristates the outputs for the whole frame:
//! as under the undervoltage lockout (see "Supply") the bus holds the last word of the
//! frame before, rounded to its code, and every sample of the frame, optical black
//! included, carries it, while the analog chain runs on behind the outputs.
//!
//! **Not modelled.** The timing core's registers: on the chip a misdirected write
//! that reached them would upset the frame's clocks, here it is lost. Also the
//! registers the chip updates on SL rather than VD, the AD9949's actual register
//! addresses and field layout, and the clamp's settling after a power-down.
//!
//! **Cost.** Seven writes of 32 bits a frame: a few hundred draws and a register
//! decode per frame, negligible beside the chain's per-sample work. A pixel gain off
//! 0 dB adds a multiply per sample.
//!
//! # Blanking
//!
//! During horizontal and vertical blanking the CCD delivers no charge. Over each
//! blanking interval the node relaxes in closed form towards the input at the
//! interval's end (the amplified injection and flicker voltages), which is exact up
//! to the residual `exp(-duration / tau)`; the flicker process takes its exact
//! transition over the interval, and both oscillator phases advance exactly with
//! time. Patterns therefore roll on from line to line and frame to frame without
//! restarting.
//!
//! A frame lasts the processor's [`Params::frame_period_s`], VD to VD, which restarts
//! the timing generator: the oscillators advance by it from one frame to the next, and
//! the vertical blanking runs until VD. A reclocked timing generator
//! ([`capture`](crate::capture)) that finishes its lines early waits in blanking
//! longer; one too slow for the frame has no blanking left, the lines it reads past VD
//! being lost to the processor. Without `[bend.clock]` the two frames are one.
//!
//! # Time scaling
//!
//! Bandwidth and slew rate describe the analog hardware in seconds at the camera's
//! native resolution. At match-input resolution the pixel period differs, so both are
//! scaled by [`Params::resolution_scale`], `dsp_pixel_period_s / native_pixel_period_s`
//! ([`Timing::resolution_scale`]), before use, which keeps the edge softness the same
//! in pixels. A reclocked timing generator does not enter the scale: the chip's pole
//! and slew stay where the hardware puts them while the sensor's pixel period moves,
//! so a slower oscillator settles each sample more (sharper edges, fewer slew limits)
//! and a faster one less. The flicker corner and the oscillator frequencies describe
//! time scales of hundreds of pixels or more and are used as given: match-input
//! timing keeps the active-line time and the frame period native, so they span the
//! same fraction of the image at every resolution. For the same reason the flicker
//! level is pinned to the white floor at the native pixel period. The restart time
//! constant is given in native lines and converted to seconds with the native line
//! period, so it spans the same fraction of the frame at every resolution.
//!
//! The runs of missed reset pulses keep their length in time, not in pixels:
//! [`ResetGate::hold_s`] is `hold_px * native_pixel_period_s`, `hold_px` periods of
//! the camera's own clock at every resolution and under a reclocked timing generator,
//! and their rate `drop_hz` is physical. Since the line time is native at every size,
//! a frame holds the same number of runs per line, covering the same fraction of each
//! line, at native and at match-input resolution: a preview is the native look
//! downscaled, and a run spans `hold_px * cols / native_width` pixels (at least one
//! period). This is the choice the readout makes for its vertical faults, whose
//! durations are native lines scaled to the virtual rows.

use std::f64::consts::TAU;
use std::fmt::{self, Debug, Formatter};
#[cfg(feature = "test-util")]
use std::ops::Range;

use crate::noise::{self, Pcg32};
use crate::power::Rail;
use crate::readout::{Layout, Stream};
use crate::scene::Channel;
use crate::sensor::Cfa;
use crate::{Camera, Error, Preset, Timing};

/// Salt of the analog front end's noise sources.
const SALT: u32 = noise::salt("afe");

/// Salt of the injected sample-and-hold oscillator's levels.
const INJECT_SALT: u32 = noise::salt("afe.inject");

/// Salt of the gain-modulating sample-and-hold oscillator's levels.
const GAIN_SALT: u32 = noise::salt("afe.gain_mod");

/// Salt of the reset gate's missed pulses, which have their own stage seed.
const RESET_SALT: u32 = noise::salt("reset");

/// Salt of the serial link's bit errors, which have their own stage seed.
const SERIAL_SALT: u32 = noise::salt("serial");

/// Bits of the register address that starts every serial write.
///
/// C: the AD9949 class sends an 8-bit address before each data field
/// (`docs/research/a520-technical.md`, section 5). An address that a flip moves past
/// the modelled registers lands nowhere.
pub const ADDRESS_BITS: u8 = 8;

/// Number of modelled serial registers, at the addresses `0` to `REGISTERS - 1`.
pub const REGISTERS: usize = 7;

/// Address of the control register: power-down and output disable.
///
/// PROVISIONAL, as every address of the register map: the model's own, not the
/// AD9949's (see the [module documentation](self), "Serial interface").
pub const CONTROL_ADDRESS: u8 = 0x00;

/// Address of the VGA gain register.
///
/// PROVISIONAL, as [`CONTROL_ADDRESS`].
pub const VGA_ADDRESS: u8 = 0x01;

/// Addresses of the four pixel gain registers, in the order R, Gr, Gb, B.
///
/// PROVISIONAL, as [`CONTROL_ADDRESS`]. The order is that of `[afe] pxga_gain_db`.
pub const PXGA_ADDRESSES: [u8; 4] = [0x02, 0x03, 0x04, 0x05];

/// Address of the black clamp level register.
///
/// PROVISIONAL, as [`CONTROL_ADDRESS`].
pub const CLAMP_ADDRESS: u8 = 0x06;

/// Bit of the control register that powers the chip down; its outputs tristate.
pub const POWER_DOWN: u32 = 1 << 0;

/// Bit of the control register that disables the chip's outputs; they tristate.
pub const OUTPUT_DISABLE: u32 = 1 << 1;

/// Width of the control register in bits: the two modelled control bits.
///
/// PROVISIONAL: the AD9949's control registers hold more fields (standby modes, test
/// modes, the timing core's enables), which the model does not hold; a data bit above
/// these two is truncated like any bit beyond a register's width.
const CONTROL_BITS: u8 = 2;

/// Scale that maps a 32-bit integer onto `[0, 2)`.
const TWO_OVER_2_32: f64 = 2.0 / 4_294_967_296.0;

/// Scale that maps a 32-bit integer onto `[0, 1)`.
const ONE_OVER_2_32: f64 = 1.0 / 4_294_967_296.0;

/// Pedestal error below which the clamp's restart after a lockout is complete, in volts.
///
/// A ten-thousandth of a 12-bit code of a 1 V converter (244 µV): far below the
/// conversion dither, so ending the relaxation there changes no code.
const RESTART_SETTLED_V: f64 = 2.4e-8;

/// Waveform of an injected oscillator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Wave {
    /// A sinusoid.
    Sine,
    /// A 50 % duty-cycle square wave, as from a 555 timer.
    #[default]
    Square,
    /// A rising sawtooth.
    Saw,
    /// Sample-and-hold noise: a new uniform random level every cycle.
    Noise,
}

/// An oscillator coupled into the analog signal path at the AFE input.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Inject {
    /// Shape of the waveform.
    pub wave: Wave,
    /// Oscillator frequency in hertz.
    pub freq_hz: f64,
    /// Peak amplitude at the AFE input in volts; the programmable gain amplifies it.
    pub amplitude_v: f64,
    /// Extra phase step per line in cycles, which tilts the bands.
    #[cfg_attr(feature = "serde", serde(default))]
    pub phase_skew: f64,
}

/// An oscillator modulating the programmable gain.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct GainMod {
    /// Shape of the waveform.
    pub wave: Wave,
    /// Oscillator frequency in hertz.
    pub freq_hz: f64,
    /// Relative gain swing; the gain moves within `1 ± depth` times its setting.
    pub depth: f64,
}

/// The reset gate's missed and weak pulses, resolved from the preset's `[bend.reset]`.
///
/// See the [module documentation](self), "Reset gate".
#[derive(Debug, Clone, PartialEq)]
pub struct ResetGate {
    /// Mean rate of runs of missed pulses per second of readout.
    pub drop_hz: f64,
    /// Mean length of a run of missed pulses in seconds.
    ///
    /// The preset's `hold_px` pixel periods of the camera's own clock, at every
    /// resolution: a run covers the same fraction of a line at native and at
    /// match-input resolution.
    pub hold_s: f64,
    /// Fraction of the diffusion's charge a reset pulse clears; 1 is a full reset.
    pub reset_frac: f64,
    /// Seed of the missed pulses, the bend's own stage seed.
    pub seed: u64,
}

/// The chip's serially programmed registers and the bit errors of their link.
///
/// Registers are indexed by address. See the [module documentation](self), "Serial
/// interface".
#[derive(Debug, Clone, PartialEq)]
pub struct Serial {
    /// Bits of the data field of a write, after its [`ADDRESS_BITS`]-bit address.
    pub data_bits: u8,
    /// Width of each register in bits.
    pub widths: [u8; REGISTERS],
    /// The camera's content of each register as a code.
    ///
    /// A register holding its nominal code applies the camera's exact value.
    pub nominal: [u32; REGISTERS],
    /// Gain of the VGA at code 0, in decibels.
    pub vga_min_db: f64,
    /// Gain step of the VGA per code, in decibels.
    pub vga_db_per_code: f64,
    /// Gain of a pixel gain register at its largest code, in decibels.
    pub pxga_max_db: f64,
    /// Probability that a bit of a write reads wrong at the chip; 0 on a sound link.
    pub bit_error: f64,
    /// Seed of the bit errors, the bend's own stage seed.
    pub seed: u64,
}

impl Serial {
    /// Resolves the register map from the camera and the bend's error rate.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a nominal value does not fit its register.
    fn resolve(camera: &Camera, bit_error: f64, seed: u64) -> Result<Self, Error> {
        let afe = &camera.afe;
        let pxga_top = f64::from(low_mask(afe.pxga_bits));
        let mut widths = [afe.pxga_bits; REGISTERS];
        widths[usize::from(CONTROL_ADDRESS)] = CONTROL_BITS;
        widths[usize::from(VGA_ADDRESS)] = afe.vga_bits;
        widths[usize::from(CLAMP_ADDRESS)] = afe.clamp_bits;
        let mut nominal = [0; REGISTERS];
        nominal[usize::from(VGA_ADDRESS)] = register_code(
            &format!("afe.pga_gain_db {} dB on the VGA", afe.pga_gain_db),
            (afe.pga_gain_db - afe.vga_min_db) / afe.vga_db_per_code,
            afe.vga_bits,
        )?;
        for ((address, gain_db), phase) in PXGA_ADDRESSES
            .into_iter()
            .zip(afe.pxga_gain_db)
            .zip(["r", "gr", "gb", "b"])
        {
            nominal[usize::from(address)] = register_code(
                &format!("afe.pxga_gain_db ({phase}) {gain_db} dB on a pixel gain register"),
                gain_db / afe.pxga_max_db * pxga_top,
                afe.pxga_bits,
            )?;
        }
        let code_v = camera.adc.full_scale_v / f64::from(camera.max_code());
        nominal[usize::from(CLAMP_ADDRESS)] = register_code(
            &format!("afe.pedestal_v {} V on the clamp", afe.pedestal_v),
            afe.pedestal_v / code_v,
            afe.clamp_bits,
        )?;
        Ok(Self {
            data_bits: afe.serial_data_bits,
            widths,
            nominal,
            vga_min_db: afe.vga_min_db,
            vga_db_per_code: afe.vga_db_per_code,
            pxga_max_db: afe.pxga_max_db,
            bit_error,
            seed,
        })
    }

    /// Returns the bits that read wrong in frame `index`'s writes, by intended address.
    ///
    /// Each entry holds the flipped address bits and the flipped data bits of one write.
    fn flips(&self, index: u64) -> [(u8, u32); REGISTERS] {
        let mut rng = Pcg32::new(self.seed, index);
        let mut field = |bits: u8| {
            let mut mask = 0u32;
            for bit in 0..bits.min(32) {
                if f64::from(rng.next_u32()) * ONE_OVER_2_32 < self.bit_error {
                    mask |= 1 << bit;
                }
            }
            mask
        };
        let mut flips = [(0, 0); REGISTERS];
        for flip in &mut flips {
            let address = field(ADDRESS_BITS) as u8;
            *flip = (address, field(self.data_bits));
        }
        flips
    }

    /// Returns the registers after frame `index`'s writes onto `held`, the last frame's.
    ///
    /// `None` stands for every register at its nominal code, as at power-on.
    fn program(&self, held: Option<[u32; REGISTERS]>, index: u64) -> Option<[u32; REGISTERS]> {
        if self.bit_error <= 0.0 {
            // A sound link: every write lands clean and restores the camera's value.
            return None;
        }
        let mut codes = held.unwrap_or(self.nominal);
        for (sent, (address_flips, data_flips)) in self.flips(index).into_iter().enumerate() {
            let landed = usize::from(sent as u8 ^ address_flips);
            if let Some(code) = codes.get_mut(landed) {
                *code = (self.nominal[sent] ^ data_flips) & low_mask(self.widths[landed]);
            }
        }
        (codes != self.nominal).then_some(codes)
    }
}

/// Returns a mask of the low `bits` bits.
fn low_mask(bits: u8) -> u32 {
    u32::MAX
        .checked_shr(32u32.saturating_sub(u32::from(bits)))
        .unwrap_or(0)
}

/// Returns the code nearest `code` if a register of `bits` bits holds it.
///
/// # Errors
///
/// Returns a configuration error naming `what` otherwise.
fn register_code(what: &str, code: f64, bits: u8) -> Result<u32, Error> {
    let top = low_mask(bits);
    let nearest = code.round();
    if (0.0..=f64::from(top)).contains(&nearest) {
        Ok(nearest as u32)
    } else {
        Err(Error::config(format!(
            "{what} needs code {nearest}, beyond the {bits}-bit register's 0 to {top}"
        )))
    }
}

/// Returns the linear gain of `gain_db` decibels.
fn db_gain(gain_db: f64) -> f64 {
    10f64.powf(gain_db / 20.0)
}

/// Returns the pixel gain register, 0 to 3 for R, Gr, Gb, B, of the photosite at `x`, `y`.
fn pxga_phase(cfa: Cfa, x: u32, y: u32) -> usize {
    match cfa.channel(x, y) {
        Channel::Red => 0,
        Channel::Green if cfa.channel(x ^ 1, y) == Channel::Red => 1,
        Channel::Green => 2,
        Channel::Blue => 3,
    }
}

/// What the registers set for one frame.
#[derive(Debug, Clone, Copy)]
struct Setting {
    /// Linear gain of the VGA.
    gain: f64,
    /// Pixel gain factors by image-row parity and column parity; `None` when all are 1.
    pxga: Option<[[f64; 2]; 2]>,
    /// The clamp's pedestal at the nominal rail, in volts.
    pedestal_v: f64,
    /// Whether the outputs are tristated for the frame.
    tristated: bool,
}

impl Setting {
    /// Decodes the registers, `None` for every one at its nominal code.
    fn new(params: &Params, registers: Option<&[u32; REGISTERS]>) -> Self {
        let serial = &params.serial;
        let codes = registers.unwrap_or(&serial.nominal);
        let code = |address: u8| codes[usize::from(address)];
        let exact = |address: u8| code(address) == serial.nominal[usize::from(address)];
        let vga_db = if exact(VGA_ADDRESS) {
            params.pga_gain_db
        } else {
            serial.vga_min_db + serial.vga_db_per_code * f64::from(code(VGA_ADDRESS))
        };
        let pxga_top = f64::from(low_mask(serial.widths[usize::from(PXGA_ADDRESSES[0])]));
        let mut factors = [1.0; 4];
        for ((factor, address), camera_db) in factors
            .iter_mut()
            .zip(PXGA_ADDRESSES)
            .zip(params.pxga_gain_db)
        {
            let gain_db = if exact(address) {
                camera_db
            } else {
                serial.pxga_max_db * f64::from(code(address)) / pxga_top
            };
            *factor = db_gain(gain_db);
        }
        // At 0 dB a factor is exactly 1, and the step is skipped.
        let unity = factors.iter().all(|f| f.to_bits() == 1f64.to_bits());
        let pxga = (!unity).then(|| {
            let at = |x: u32, y: u32| factors[pxga_phase(params.cfa, x, y)];
            [[at(0, 0), at(1, 0)], [at(0, 1), at(1, 1)]]
        });
        let pedestal_v = if exact(CLAMP_ADDRESS) {
            params.pedestal_v
        } else {
            f64::from(code(CLAMP_ADDRESS)) * params.code_v
        };
        Self {
            gain: db_gain(vga_db),
            pxga,
            pedestal_v,
            tristated: code(CONTROL_ADDRESS) & (POWER_DOWN | OUTPUT_DISABLE) != 0,
        }
    }
}

/// Voltages at the ADC input in readout order.
#[derive(Clone, PartialEq)]
pub struct Analog {
    volts: Box<[f32]>,
    index: u64,
    layout: Layout,
}

impl Analog {
    /// Creates a buffer of zero volts with the given layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            volts: vec![0.0; layout.sample_count()].into_boxed_slice(),
            index: 0,
            layout,
        }
    }

    /// Returns all voltages, line by line.
    #[must_use]
    pub fn volts(&self) -> &[f32] {
        &self.volts
    }

    /// Returns all voltages mutably.
    pub fn volts_mut(&mut self) -> &mut [f32] {
        &mut self.volts
    }

    /// Returns the index of the frame the voltages belong to.
    #[must_use]
    pub fn index(&self) -> u64 {
        self.index
    }

    /// Returns the geometry and timing of the samples.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Gives the voltages the timing of `layout`, keeping the samples.
    ///
    /// # Panics
    ///
    /// Panics if `layout` differs from the buffer's own in anything but its pixel period.
    pub(crate) fn retime(&mut self, layout: Layout) {
        assert!(
            self.layout.same_shape(&layout),
            "cannot retime voltages of layout {:?} to {layout:?}: only the clock may change",
            self.layout
        );
        self.layout = layout;
    }
}

impl Debug for Analog {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analog")
            .field("index", &self.index)
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// Parameters of the analog chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Output amplifier conversion gain in microvolts per electron.
    pub conv_gain_uv_per_e: f64,
    /// White read noise in electrons, one standard deviation per sample.
    pub read_noise_e: f64,
    /// Read-noise corner in hertz, below which flicker exceeds the white floor; 0 disables it.
    pub noise_1f_corner_hz: f64,
    /// Programmable gain amplifier gain in decibels: the camera's VGA setting.
    pub pga_gain_db: f64,
    /// Gains of the four pixel gain amplifiers in decibels, in the order R, Gr, Gb, B.
    ///
    /// The camera's `PxGA` settings; each multiplies the samples of its mosaic phase.
    pub pxga_gain_db: [f64; 4],
    /// Layout of the colour filter array, which gives each sample its pixel gain.
    pub cfa: Cfa,
    /// Voltage at the ADC input for zero charge: the camera's clamp level.
    pub pedestal_v: f64,
    /// Small-signal bandwidth at the native resolution in hertz.
    pub bandwidth_hz: f64,
    /// Slew-rate limit at the native resolution in volts per microsecond.
    pub slew_v_per_us: f64,
    /// Duration of one pixel period of the camera's own clock in seconds.
    pub native_pixel_period_s: f64,
    /// Factor that stretches the bandwidth's time constant and the slew time to the resolution.
    ///
    /// [`Timing::resolution_scale`], `dsp_pixel_period_s / native_pixel_period_s`: 1 at
    /// native resolution whatever the sensor-side clock.
    pub resolution_scale: f64,
    /// Duration of the processor's frame, VD to VD, in seconds.
    ///
    /// The oscillators advance by it from one frame to the next.
    pub frame_period_s: f64,
    /// Simulation sub-steps per pixel period.
    pub oversample: u32,
    /// Nominal ADC reference: the input voltage of the largest code.
    pub vref_v: f64,
    /// Input voltage of one ADC code: `vref_v / (2^bits - 1)`.
    pub code_v: f64,
    /// Supply voltage below which the chip locks out and the bus holds its last word.
    pub uvlo_v: f64,
    /// Time constant of the black clamp re-establishing the pedestal after a lockout, in seconds.
    pub restart_s: f64,
    /// Delay of the CDS reset sample in pixel periods.
    pub cds_lag_px: f64,
    /// Fraction of the delayed level that the late reset sample subtracts.
    pub cds_mix: f64,
    /// Swing of the CCD's output stage in volts.
    ///
    /// Charge on the floating diffusion beyond `fd_swing_v / conv_gain` no longer moves
    /// the output.
    pub fd_swing_v: f64,
    /// Missed and weak reset pulses, if the preset has a `[bend.reset]` that is not inert.
    pub reset: Option<ResetGate>,
    /// The serially programmed registers and the `[bend.serial]` bit errors on their link.
    pub serial: Serial,
    /// Injected oscillator, if the preset patches one in.
    pub inject: Option<Inject>,
    /// Gain-modulating oscillator, if the preset patches one in.
    pub gain_mod: Option<GainMod>,
    /// Seed of the analog noise sources.
    pub seed: u64,
}

impl Params {
    /// Resolves the analog parameters from the camera, preset and derived timing.
    ///
    /// The restart time constant is the camera's `afe_restart_lines` in native lines
    /// of the readout mode, converted to seconds with the camera's own pixel clock.
    /// An inert `[bend.reset]` resolves to no [`ResetGate`]; a live one's run length
    /// becomes seconds on the camera's own clock. The camera's VGA gain, pixel gains
    /// and pedestal become the nominal codes of their registers ([`Serial`]); without
    /// `[bend.serial]` the link is sound.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range, or if the camera's
    /// VGA gain, a pixel gain or its pedestal does not fit its serial register.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let afe = &camera.afe;
        let bend = preset.bend.afe.clone().unwrap_or_default();
        let clock = camera.clocking(timing.mode);
        let native_line_px = camera.sensor.ob_cols + camera.sensor.native_width + clock.hblank_px;
        let native_line_s = f64::from(native_line_px) / clock.pixel_clock_hz;
        let reset = preset
            .bend
            .reset
            .filter(|bend| !bend.is_inert())
            .map(|bend| ResetGate {
                drop_hz: bend.drop_hz,
                hold_s: bend.hold_px * timing.native_pixel_period_s,
                reset_frac: bend.reset_frac,
                seed: noise::stage_seed(preset.seed, RESET_SALT),
            });
        let serial = Serial::resolve(
            camera,
            preset.bend.serial.map_or(0.0, |bend| bend.bit_error),
            noise::stage_seed(preset.seed, SERIAL_SALT),
        )?;
        Ok(Self {
            conv_gain_uv_per_e: camera.sensor.conv_gain_uv_per_e,
            read_noise_e: camera.sensor.read_noise_e,
            noise_1f_corner_hz: afe.noise_1f_corner_hz,
            pga_gain_db: afe.pga_gain_db,
            pxga_gain_db: afe.pxga_gain_db,
            cfa: camera.sensor.cfa,
            pedestal_v: afe.pedestal_v,
            bandwidth_hz: afe.bandwidth_hz,
            slew_v_per_us: afe.slew_v_per_us,
            native_pixel_period_s: timing.native_pixel_period_s,
            resolution_scale: timing.resolution_scale(),
            frame_period_s: timing.frame_period_s,
            oversample: timing.oversample,
            vref_v: camera.adc.full_scale_v,
            code_v: camera.adc.full_scale_v / f64::from(camera.max_code()),
            uvlo_v: camera.power.afe_uvlo_v,
            restart_s: f64::from(camera.power.afe_restart_lines) * native_line_s,
            cds_lag_px: bend.cds_lag_px,
            cds_mix: bend.cds_mix,
            fd_swing_v: afe.fd_swing_v,
            reset,
            serial,
            inject: bend.inject,
            gain_mod: bend.gain_mod,
            seed: noise::stage_seed(preset.seed, SALT),
        })
    }

    /// Returns the linear gain of the programmable gain amplifier.
    #[must_use]
    pub fn pga_gain(&self) -> f64 {
        db_gain(self.pga_gain_db)
    }

    /// Returns the total gain from electrons to volts at the ADC input.
    #[must_use]
    pub fn volts_per_electron(&self) -> f64 {
        self.conv_gain_uv_per_e * 1e-6 * self.pga_gain()
    }
}

/// An oscillator phase: whole cycles since power-on and the fraction of the current one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Phase {
    cycle: u64,
    frac: f64,
}

impl Phase {
    /// Returns the phase `cycles` later.
    fn advanced(self, cycles: f64) -> Self {
        let total = self.frac + cycles;
        let whole = total.floor();
        let mut next = Self {
            cycle: self.cycle.wrapping_add_signed(whole as i64),
            frac: total - whole,
        };
        if next.frac >= 1.0 {
            next.frac -= 1.0;
            next.cycle = next.cycle.wrapping_add(1);
        }
        next
    }
}

/// Persistent analog state carried from one frame to the next.
///
/// Holds the low-pass node, the flicker-noise process, the phases of both
/// oscillators, the reference the supply delivered, the chip's power state (the last
/// output, whether it is locked out and the pedestal while the clamp restarts), the
/// CCD's floating diffusion (its charge and whether its reset pulses are being
/// missed) and the serially programmed registers. The default is the power-on state,
/// an empty diffusion and the camera's register values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    node_v: f64,
    flicker_v: f64,
    inject: Phase,
    gain: Phase,
    rail_v: f64,
    /// The last output in volts; while locked out, the held word's voltage.
    last_v: f64,
    /// Whether the supply was below the lockout at the last sample.
    locked: bool,
    /// The pedestal in volts while the clamp re-establishes it; `None` once settled.
    restart_v: Option<f64>,
    /// Charge on the CCD's floating diffusion in electrons.
    fd_charge_e: f64,
    /// Whether the reset pulse was missing in the frame's last pixel period.
    reset_missed: bool,
    /// The serial registers' codes by address; `None` while all hold the camera's values.
    serial: Option<[u32; REGISTERS]>,
}

impl State {
    /// Returns the injected oscillator's phase at the start of the next frame, in cycles.
    ///
    /// Always in `[0, 1)`.
    #[must_use]
    pub fn phase(&self) -> f64 {
        self.inject.frac
    }

    /// Returns the low-pass node voltage above the pedestal after the last frame.
    #[must_use]
    pub fn node_v(&self) -> f64 {
        self.node_v
    }

    /// Returns the ADC reference the supply delivered at the end of the last frame, in volts.
    ///
    /// Zero before the first frame. [`Params::vref_v`] at the nominal rail; it follows
    /// the rail ratiometrically, `vref_v * rail / nominal`.
    #[must_use]
    pub fn rail_v(&self) -> f64 {
        self.rail_v
    }

    /// Returns whether the chip was locked out by an undervoltage at the end of the last frame.
    #[must_use]
    pub fn locked_out(&self) -> bool {
        self.locked
    }

    /// Returns the pedestal while the black clamp re-establishes it after a lockout, in volts.
    ///
    /// `None` once it has settled onto the rail's pedestal, as normally.
    #[must_use]
    pub fn restart_pedestal_v(&self) -> Option<f64> {
        self.restart_v
    }

    /// Returns the charge on the CCD's floating diffusion after the last frame, in electrons.
    ///
    /// With a sound reset gate it is the frame's last packet, which the next line's
    /// first reset clears; missed and weak resets let it build up.
    #[must_use]
    pub fn fd_charge_e(&self) -> f64 {
        self.fd_charge_e
    }

    /// Returns whether the reset pulses were being missed at the end of the last frame.
    #[must_use]
    pub fn reset_missed(&self) -> bool {
        self.reset_missed
    }

    /// Returns the codes of the serial registers after the last frame's writes, for tests.
    ///
    /// Indexed by address, [`CONTROL_ADDRESS`] to [`CLAMP_ADDRESS`]. `None` while every
    /// register holds its nominal code and so the camera's value, as at power-on and
    /// after a frame of clean writes.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn serial_codes(&self) -> Option<[u32; REGISTERS]> {
        self.serial
    }
}

/// One oscillator of the chain with the seed of its sample-and-hold levels.
#[derive(Debug, Clone, Copy)]
struct Osc {
    wave: Wave,
    freq_hz: f64,
    skew: f64,
    salt: u32,
    seed: u64,
}

impl Osc {
    /// Returns the phase at time `t_s` of the frame, in line `line`, from the frame's start phase.
    fn phase(&self, start: Phase, t_s: f64, line: u32) -> Phase {
        start.advanced(self.freq_hz * t_s + self.skew * f64::from(line))
    }

    /// Returns a generator of the waveform from `phase` in steps of `step_s` seconds.
    fn sweep(&self, phase: Phase, step_s: f64) -> Sweep {
        let step = self.freq_hz * step_s;
        let (sin, cos) = (TAU * phase.frac).sin_cos();
        let (rot_sin, rot_cos) = (TAU * step).sin_cos();
        Sweep {
            osc: *self,
            base: phase.cycle,
            offset: phase.frac,
            step,
            phasor: (cos, sin),
            rotation: (rot_cos, rot_sin),
            held: None,
        }
    }

    /// Returns the waveform's value, in `[-1, 1]`, at `phase`.
    fn value(&self, phase: Phase) -> f64 {
        self.sweep(phase, 0.0).next()
    }
}

/// An oscillator's waveform sampled at evenly spaced instants.
///
/// The sine is generated by rotating a phasor, the other shapes from the phase.
struct Sweep {
    osc: Osc,
    /// Whole cycles at the first instant.
    base: u64,
    /// Phase of the next instant in cycles past `base`; never negative.
    offset: f64,
    /// Phase advance per instant in cycles.
    step: f64,
    /// Cosine and sine of the next instant's phase.
    phasor: (f64, f64),
    /// Cosine and sine of one step.
    rotation: (f64, f64),
    /// The current sample-and-hold cycle and its level.
    held: Option<(u64, f64)>,
}

impl Sweep {
    /// Returns the waveform at the next instant, in `[-1, 1]`.
    fn next(&mut self) -> f64 {
        let value = match self.osc.wave {
            Wave::Sine => {
                let (cos, sin) = self.phasor;
                let (rot_cos, rot_sin) = self.rotation;
                self.phasor = (cos * rot_cos - sin * rot_sin, sin * rot_cos + cos * rot_sin);
                sin
            }
            Wave::Square => {
                if self.offset.fract() < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Saw => 2.0 * self.offset.fract() - 1.0,
            Wave::Noise => {
                let cycle = self.base.wrapping_add(self.offset as u64);
                match self.held {
                    Some((held, level)) if held == cycle => level,
                    _ => {
                        let level = f64::from(noise::signed(
                            cycle as u32,
                            (cycle >> 32) as u32,
                            self.osc.salt,
                            self.osc.seed,
                        ));
                        self.held = Some((cycle, level));
                        level
                    }
                }
            }
        };
        self.offset += self.step;
        value
    }
}

/// Returns two independent standard normal values (Marsaglia's polar method).
fn normal_pair(rng: &mut Pcg32) -> (f64, f64) {
    loop {
        let u = f64::from(rng.next_u32()) * TWO_OVER_2_32 - 1.0;
        let v = f64::from(rng.next_u32()) * TWO_OVER_2_32 - 1.0;
        let s = u * u + v * v;
        if s > 0.0 && s < 1.0 {
            let m = (-2.0 * s.ln() / s).sqrt();
            return (u * m, v * m);
        }
    }
}

/// Read noise at the AFE input: white plus first-order flicker, in volts.
struct ReadNoise {
    rng: Pcg32,
    enabled: bool,
    white_v: f64,
    flicker_sigma_v: f64,
    corner_hz: f64,
    /// Flicker decay per pixel period.
    decay: f64,
    /// Flicker innovation per pixel period, in volts per unit normal.
    kick_v: f64,
    flicker_v: f64,
}

impl ReadNoise {
    fn new(params: &Params, pixel_s: f64, index: u64, flicker_v: f64) -> Self {
        let white_v = params.read_noise_e * params.conv_gain_uv_per_e * 1e-6;
        let corner_hz = params.noise_1f_corner_hz;
        // The white floor's one-sided density is 2 white_v^2 T at the native period T.
        // A first-order process with density 2 * 2 white_v^2 T at DC, and so the
        // floor's density at its corner, has variance 2 pi corner T white_v^2.
        let flicker_sigma_v = white_v * (TAU * corner_hz * params.native_pixel_period_s).sqrt();
        let decay = (-TAU * corner_hz * pixel_s).exp();
        Self {
            rng: Pcg32::new(params.seed, index),
            enabled: white_v > 0.0 || flicker_sigma_v > 0.0,
            white_v,
            flicker_sigma_v,
            corner_hz,
            decay,
            kick_v: flicker_sigma_v * (1.0 - decay * decay).sqrt(),
            flicker_v,
        }
    }

    /// Returns the noise of the next sample.
    fn sample(&mut self) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        let (white, flicker) = normal_pair(&mut self.rng);
        self.flicker_v = self.flicker_v * self.decay + self.kick_v * flicker;
        self.white_v * white + self.flicker_v
    }

    /// Advances the flicker process over a blanking interval with its exact transition.
    fn skip(&mut self, duration_s: f64) {
        if self.flicker_sigma_v > 0.0 {
            let decay = (-TAU * self.corner_hz * duration_s).exp();
            let (kick, _) = normal_pair(&mut self.rng);
            self.flicker_v =
                self.flicker_v * decay + self.flicker_sigma_v * (1.0 - decay * decay).sqrt() * kick;
        }
    }
}

/// Returns a geometric number of pixel periods, at least 1, of a stretch of reset pulses.
///
/// `stay_ln` is the logarithm of the probability that the stretch goes on past a
/// period, `ln(1 - p)` for an end probability `p`; 0 is a stretch that never ends.
fn stretch_periods(rng: &mut Pcg32, stay_ln: f64) -> u64 {
    if stay_ln == 0.0 {
        return u64::MAX;
    }
    // Inverse CDF of the geometric distribution on {1, 2, ...} from a uniform in (0, 1].
    let unit = (f64::from(rng.next_u32()) + 1.0) * ONE_OVER_2_32;
    ((unit.ln() / stay_ln).floor() as u64).saturating_add(1)
}

/// The reset gate's pulses over one frame: stretches of sound pulses and runs of missed ones.
#[derive(Clone)]
struct Resets {
    rng: Pcg32,
    /// Logarithm of the probability that a stretch goes on past a period: sound, missed.
    stay_ln: [f64; 2],
    /// Whether the pulses of the current stretch are missed.
    missed: bool,
    /// Pixel periods left in the current stretch.
    left: u64,
}

impl Resets {
    /// Starts frame `index` with a run of missed pulses in progress if `missed`.
    fn new(gate: &ResetGate, pixel_s: f64, missed: bool, index: u64) -> Self {
        // A run starts with probability drop_hz * pixel_s in a period while none runs
        // and ends with probability 1 / mean in each of its periods.
        let start_p = (gate.drop_hz * pixel_s).min(1.0);
        let mean_px = (gate.hold_s / pixel_s).max(1.0);
        let stay_ln = [(-start_p).ln_1p(), (-1.0 / mean_px).ln_1p()];
        let mut rng = Pcg32::new(gate.seed, index);
        // Without drops the contact is sound: a run in progress ends at once.
        let missed = missed && gate.drop_hz > 0.0;
        // The stretch in progress goes on for a geometric number of periods from zero.
        let left = stretch_periods(&mut rng, stay_ln[usize::from(missed)]) - 1;
        Self {
            rng,
            stay_ln,
            missed,
            left,
        }
    }

    /// Returns whether the reset pulse arrives in the next pixel period, and moves past it.
    fn arrives(&mut self) -> bool {
        if self.left == 0 {
            self.turn();
        }
        self.left -= 1;
        !self.missed
    }

    /// Moves past `periods` pixel periods at once, as [`arrives`](Self::arrives) that many times.
    fn skip(&mut self, mut periods: u64) {
        while periods > self.left {
            periods -= self.left;
            self.turn();
        }
        self.left -= periods;
    }

    /// Ends the current stretch and draws the length of the next.
    fn turn(&mut self) {
        self.missed = !self.missed;
        self.left = stretch_periods(&mut self.rng, self.stay_ln[usize::from(self.missed)]);
    }
}

/// The CCD's floating diffusion and its reset gate.
#[derive(Clone)]
struct Diffusion {
    resets: Resets,
    /// Fraction of the charge a reset pulse clears.
    reset_frac: f64,
    /// The output stage's swing in electrons.
    swing_e: f64,
    /// Charge on the diffusion in electrons.
    charge_e: f64,
}

impl Diffusion {
    fn new(params: &Params, gate: &ResetGate, state: &State, pixel_s: f64, index: u64) -> Self {
        Self {
            resets: Resets::new(gate, pixel_s, state.reset_missed, index),
            reset_frac: gate.reset_frac,
            swing_e: params.fd_swing_v / (params.conv_gain_uv_per_e * 1e-6),
            charge_e: state.fd_charge_e,
        }
    }

    /// Returns what the CDS reads of a packet of `packet_e` electrons, in electrons.
    ///
    /// Advances the diffusion and its reset gate by one pixel period.
    fn read(&mut self, packet_e: f64) -> f64 {
        if self.resets.arrives() {
            self.charge_e -= self.reset_frac * self.charge_e;
        }
        let reset_e = self.charge_e.min(self.swing_e);
        self.charge_e += packet_e;
        self.charge_e.min(self.swing_e) - reset_e
    }
}

/// The diffusion of the line replayed behind the CDS, for its late reset sample.
struct Echo {
    diffusion: Diffusion,
    /// The next sample of the line to replay.
    next: usize,
    /// What the CDS read of the last replayed sample, in electrons.
    read_e: f64,
}

/// The chip's supply: the rail's ratio to nominal, the lockout and the clamp's restart.
struct Power {
    /// The camera's nominal rail in volts.
    nominal_v: f64,
    /// The pedestal at the nominal rail in volts.
    pedestal_v: f64,
    /// Supply voltage below which the chip locks out.
    uvlo_v: f64,
    /// Input voltage of one ADC code.
    code_v: f64,
    /// Time constant of the clamp's restart in seconds.
    restart_s: f64,
    /// The clamp's restart decay per pixel period.
    restart_decay: f64,
    /// The last output in volts; while locked out, the held word's voltage.
    last_v: f64,
    /// Whether the chip is locked out.
    locked: bool,
    /// The pedestal while the clamp re-establishes it, in volts.
    restart_v: Option<f64>,
}

impl Power {
    /// Starts a frame whose clamp holds `pedestal_v` at the nominal rail.
    fn new(params: &Params, state: &State, pedestal_v: f64, nominal_v: f64, pixel_s: f64) -> Self {
        Self {
            nominal_v,
            pedestal_v,
            uvlo_v: params.uvlo_v,
            code_v: params.code_v,
            restart_s: params.restart_s,
            restart_decay: (-pixel_s / params.restart_s).exp(),
            last_v: state.last_v,
            locked: state.locked,
            restart_v: state.restart_v,
        }
    }

    /// Returns the rail's ratio to nominal if a line supplied from `start_v` to `end_v` takes the plain path.
    ///
    /// That is a line at one voltage above the lockout, with the clamp settled.
    fn steady(&self, [start_v, end_v]: [f64; 2]) -> Option<f64> {
        let steady = start_v.to_bits() == end_v.to_bits()
            && start_v >= self.uvlo_v
            && !self.locked
            && self.restart_v.is_none();
        steady.then(|| start_v / self.nominal_v)
    }

    /// Returns the output for the node at `node_v` with the rail at `rail_v`, in volts.
    ///
    /// Below the lockout the output holds the last word; above it the pedestal and the
    /// reference follow the rail, the pedestal through the clamp's restart after a
    /// lockout. Advances the restart by one pixel period.
    fn output(&mut self, node_v: f64, rail_v: f64) -> f64 {
        if rail_v < self.uvlo_v {
            if !self.locked {
                self.locked = true;
                self.restart_v = None;
                self.last_v = (self.last_v / self.code_v).round() * self.code_v;
            }
            return self.last_v;
        }
        let ratio = rail_v / self.nominal_v;
        let settled_v = self.pedestal_v * ratio;
        if std::mem::take(&mut self.locked) {
            self.restart_v = Some(self.pedestal_v * self.uvlo_v / self.nominal_v);
        }
        let pedestal_v = match self.restart_v {
            Some(restart_v) => {
                let next_v = settled_v + (restart_v - settled_v) * self.restart_decay;
                self.restart_v = ((next_v - settled_v).abs() > RESTART_SETTLED_V).then_some(next_v);
                next_v
            }
            None => settled_v,
        };
        self.last_v = (pedestal_v + node_v) / ratio;
        self.last_v
    }

    /// Advances the clamp's restart over a blanking interval that ends with the rail at `rail_v`.
    fn blank(&mut self, duration_s: f64, rail_v: f64) {
        if let Some(restart_v) = self.restart_v {
            let settled_v = self.pedestal_v * rail_v / self.nominal_v;
            let next_v = settled_v + (restart_v - settled_v) * (-duration_s / self.restart_s).exp();
            self.restart_v = ((next_v - settled_v).abs() > RESTART_SETTLED_V).then_some(next_v);
        }
    }
}

/// The analog chain over one frame: derived constants and the evolving node.
struct Front<'a> {
    params: &'a Params,
    volts_per_e: f64,
    /// The VGA's linear gain for the frame.
    gain: f64,
    /// Pixel gain factors by image-row parity and column parity, unless all are 1.
    pxga: Option<[[f64; 2]; 2]>,
    /// The clamp's pedestal at the nominal rail for the frame, in volts.
    pedestal_v: f64,
    /// Optical-black samples at the start of each line.
    ob_cols: usize,
    oversample: u32,
    sub_s: f64,
    tau_s: f64,
    /// Node decay per sub-step and per pixel period.
    decay: (f64, f64),
    slew_v_per_s: f64,
    /// Largest node step per sub-step in volts.
    sub_slew_v: f64,
    /// CDS reset-sample delay in sub-steps.
    lag_sub: f64,
    /// Injected oscillator, its amplitude and its phase at the frame start.
    inject: Option<(Osc, f64, Phase)>,
    /// Gain-modulating oscillator, its depth and its phase at the frame start.
    gain_mod: Option<(Osc, f64, Phase)>,
    /// Pixel periods per line, blanking included: the span of one rail entry.
    line_px: f64,
    /// The supply and what it does to the chip.
    power: Power,
    noise: ReadNoise,
    node_v: f64,
    /// The floating diffusion, while `[bend.reset]` breaks its reset pulses.
    diffusion: Option<Diffusion>,
    /// The diffusion replayed for the late reset sample of the current line.
    echo: Option<Echo>,
}

impl<'a> Front<'a> {
    fn new(
        params: &'a Params,
        state: &State,
        setting: &Setting,
        layout: &Layout,
        rail: &Rail,
        index: u64,
    ) -> Self {
        let pixel_s = layout.pixel_period_s();
        let oversample = params.oversample.max(1);
        let sub_s = pixel_s / f64::from(oversample);
        // Bandwidth and slew rate describe the hardware at the native resolution: scale
        // them to the processor's pixel period, so they keep their meaning in pixels at
        // every resolution and stay fixed in seconds under a reclocked sensor clock.
        let scale = params.resolution_scale;
        let tau_s = scale / (TAU * params.bandwidth_hz);
        let slew_v_per_s = params.slew_v_per_us * 1e6 / scale;
        let decay = (-sub_s / tau_s).exp();
        let inject = params.inject.as_ref().map(|inject| {
            let osc = Osc {
                wave: inject.wave,
                freq_hz: inject.freq_hz,
                skew: inject.phase_skew,
                salt: INJECT_SALT,
                seed: params.seed,
            };
            (osc, inject.amplitude_v, state.inject)
        });
        let gain_mod = params.gain_mod.as_ref().map(|gain_mod| {
            let osc = Osc {
                wave: gain_mod.wave,
                freq_hz: gain_mod.freq_hz,
                skew: 0.0,
                salt: GAIN_SALT,
                seed: params.seed,
            };
            (osc, gain_mod.depth, state.gain)
        });
        Self {
            params,
            volts_per_e: params.conv_gain_uv_per_e * 1e-6,
            gain: setting.gain,
            pxga: setting.pxga,
            pedestal_v: setting.pedestal_v,
            ob_cols: layout.ob_cols() as usize,
            oversample,
            sub_s,
            tau_s,
            decay: (decay, decay.powf(f64::from(oversample))),
            slew_v_per_s,
            sub_slew_v: slew_v_per_s * sub_s,
            lag_sub: params.cds_lag_px * f64::from(oversample),
            inject,
            gain_mod,
            line_px: f64::from(layout.line_len() + layout.hblank_px()),
            power: Power::new(params, state, setting.pedestal_v, rail.nominal_v(), pixel_s),
            noise: ReadNoise::new(params, pixel_s, index, state.flicker_v),
            node_v: state.node_v,
            diffusion: params
                .reset
                .as_ref()
                .map(|gate| Diffusion::new(params, gate, state, pixel_s, index)),
            echo: None,
        }
    }

    /// Moves the node one sub-step towards `target_v`, within the slew limit.
    fn settle(&mut self, target_v: f64) {
        let next = target_v + (self.node_v - target_v) * self.decay.0;
        self.node_v += (next - self.node_v).clamp(-self.sub_slew_v, self.sub_slew_v);
    }

    /// Moves the node one pixel period towards a constant `target_v`.
    fn settle_pixel(&mut self, target_v: f64) {
        // The first sub-step moves furthest; if it is within the slew limit, all are,
        // and the whole period is one exponential step.
        if (target_v - self.node_v).abs() * (1.0 - self.decay.0) <= self.sub_slew_v {
            self.node_v = target_v + (self.node_v - target_v) * self.decay.1;
        } else {
            for _ in 0..self.oversample {
                self.settle(target_v);
            }
        }
    }

    /// Returns the CCD level the late reset sample sees at sub-step `sub` of `sample`, in volts.
    ///
    /// With `[bend.reset]` the level is what the CDS read of the earlier sample, which
    /// the line's replayed diffusion gives.
    fn lagged(&mut self, electrons: &[f32], sample: usize, sub: u32) -> f64 {
        let oversample = f64::from(self.oversample);
        let at = (sample as f64 * oversample + f64::from(sub) + 0.5 - self.lag_sub) / oversample;
        if at < 0.0 {
            return 0.0;
        }
        let at = at as usize;
        let read_e = match self.echo.as_mut() {
            Some(echo) => {
                while echo.next <= at {
                    echo.read_e = echo.diffusion.read(f64::from(electrons[echo.next]));
                    echo.next += 1;
                }
                echo.read_e
            }
            None => f64::from(electrons[at]),
        };
        read_e * self.volts_per_e
    }

    /// Runs the stored samples of `line`, which starts `start_s` into the frame.
    ///
    /// `image_row` is the line's row in the image, whose parity gives the mosaic row of
    /// the pixel gains; `supply` is the rail at the start of the line and at the end of
    /// its blanking.
    fn line(
        &mut self,
        (line, image_row): (u32, u32),
        start_s: f64,
        electrons: &[f32],
        volts: &mut [f32],
        supply: [f64; 2],
    ) {
        let first_s = start_s + 0.5 * self.sub_s;
        let mut inject = self.inject.map(|(osc, amplitude, start)| {
            (
                osc.sweep(osc.phase(start, first_s, line), self.sub_s),
                amplitude,
            )
        });
        let mut gain_mod = self.gain_mod.map(|(osc, depth, start)| {
            (
                osc.sweep(osc.phase(start, first_s, line), self.sub_s),
                depth,
            )
        });
        let cds_mix = self.params.cds_mix;
        let plain = inject.is_none() && gain_mod.is_none() && cds_mix <= 0.0;
        // The late reset sample replays the line's diffusion from where it starts.
        self.echo = self
            .diffusion
            .as_ref()
            .filter(|_| cds_mix > 0.0)
            .map(|diffusion| Echo {
                diffusion: diffusion.clone(),
                next: 0,
                read_e: 0.0,
            });
        // On a steady rail the pedestal and the reference factor hold for the line.
        let steady = self.power.steady(supply);
        let (pedestal_v, reference) =
            steady.map_or((0.0, 0.0), |ratio| (self.pedestal_v * ratio, 1.0 / ratio));
        let slope_v = (supply[1] - supply[0]) / self.line_px;
        let mut last_v = self.power.last_v;
        // The pixel gains of the line's mosaic row, by the parity of the column, which
        // runs from the first active sample and continues leftwards over optical black.
        let pxga = self.pxga.map(|rows| rows[(image_row & 1) as usize]);
        let ob_cols = self.ob_cols;
        for (sample, (volts, &charge)) in volts.iter_mut().zip(electrons).enumerate() {
            let packet_e = match self.diffusion.as_mut() {
                Some(diffusion) => diffusion.read(f64::from(charge)),
                None => f64::from(charge),
            };
            let level = packet_e * self.volts_per_e + self.noise.sample();
            let gain = pxga.map_or(self.gain, |factors| {
                self.gain * factors[(sample + ob_cols) & 1]
            });
            if plain {
                self.settle_pixel(level * gain);
            } else {
                for sub in 0..self.oversample {
                    let mut input = level;
                    if cds_mix > 0.0 {
                        input -= cds_mix * self.lagged(electrons, sample, sub);
                    }
                    if let Some((sweep, amplitude)) = inject.as_mut() {
                        input += *amplitude * sweep.next();
                    }
                    let gain = gain_mod
                        .as_mut()
                        .map_or(gain, |(sweep, depth)| gain * (1.0 + *depth * sweep.next()));
                    self.settle(input * gain);
                }
            }
            last_v = if steady.is_some() {
                (pedestal_v + self.node_v) * reference
            } else {
                // The ADC samples the node at the end of the pixel period.
                let rail_v = slope_v.mul_add((sample + 1) as f64, supply[0]);
                self.power.output(self.node_v, rail_v)
            };
            *volts = last_v as f32;
        }
        self.power.last_v = last_v;
    }

    /// Advances the chain over a blanking interval of `duration_s` that ends at `end_s` in `end_line`.
    ///
    /// The rail stands at `rail_v` at the end of the interval.
    fn blank(&mut self, duration_s: f64, end_s: f64, end_line: u32, rail_v: f64) {
        self.power.blank(duration_s, rail_v);
        self.noise.skip(duration_s);
        let mut input = self.noise.flicker_v;
        if let Some((osc, amplitude, start)) = self.inject {
            input += amplitude * osc.value(osc.phase(start, end_s, end_line));
        }
        let gain = self.gain_mod.map_or(self.gain, |(osc, depth, start)| {
            self.gain * (1.0 + depth * osc.value(osc.phase(start, end_s, end_line)))
        });
        let target_v = input * gain;
        let next = target_v + (self.node_v - target_v) * (-duration_s / self.tau_s).exp();
        let limit = self.slew_v_per_s * duration_s;
        self.node_v += (next - self.node_v).clamp(-limit, limit);
    }

    /// Moves the reset gate past `periods` pixel periods of blanking, which bring no packets.
    fn skip_resets(&mut self, periods: u64) {
        if let Some(diffusion) = self.diffusion.as_mut() {
            diffusion.resets.skip(periods);
        }
    }
}

/// Returns the vertical blanking of a frame in seconds: from the last line's blanking to VD.
fn vblank_s(params: &Params, layout: &Layout) -> f64 {
    let line_s = layout.line_period_s();
    let frame_lines = layout.rows() + layout.vblank_lines();
    // VD ends the frame on the processor's clock. The timing generator's own frame of
    // `frame_lines` lines ends before it when the generator runs fast and after it when
    // slow; the vertical blanking stretches or shrinks by the difference, which is
    // exactly zero on one clock.
    (f64::from(layout.vblank_lines()) * line_s
        + (params.frame_period_s - f64::from(frame_lines) * line_s))
        .max(0.0)
}

/// Returns the vertical blanking of a frame in whole pixel periods.
fn vblank_periods(params: &Params, layout: &Layout) -> u64 {
    (vblank_s(params, layout) / layout.pixel_period_s()).round() as u64
}

/// Returns the runs of missed reset pulses that frame `index` draws from `state`, for tests.
///
/// Each run is a range of pixel periods counted from the frame's first sample,
/// blanking included: period `p` lies in line `p / (line_len + hblank_px)`, at stored
/// sample `p % (line_len + hblank_px)` if that is below `line_len` and in the line's
/// blanking otherwise, and periods from `rows * (line_len + hblank_px)` on lie in the
/// vertical blanking. A run in progress at the frame's start begins at 0; one in
/// progress at VD ends at the frame's period count. Empty without a [`ResetGate`].
#[cfg(feature = "test-util")]
#[must_use]
pub fn missed_runs(params: &Params, state: &State, layout: &Layout, index: u64) -> Vec<Range<u64>> {
    let Some(gate) = params.reset.as_ref() else {
        return Vec::new();
    };
    let line_px = u64::from(layout.line_len() + layout.hblank_px());
    let periods = u64::from(layout.rows()) * line_px + vblank_periods(params, layout);
    let mut resets = Resets::new(gate, layout.pixel_period_s(), state.reset_missed, index);
    let mut runs = Vec::new();
    let mut start = None;
    for period in 0..periods {
        match (resets.arrives(), start) {
            (false, None) => start = Some(period),
            (true, Some(first)) => {
                runs.push(first..period);
                start = None;
            }
            _ => {}
        }
    }
    runs.extend(start.map(|first| first..periods));
    runs
}

/// Returns the bits that read wrong in frame `index`'s serial writes, for tests.
///
/// One entry per write, in the order the processor sends them, which is the order of
/// the intended addresses: the mask of the flipped address bits and the mask of the
/// flipped data bits, padding included. Without `[bend.serial]` or at `bit_error = 0`
/// every mask is empty.
#[cfg(feature = "test-util")]
#[must_use]
pub fn serial_flips(params: &Params, index: u64) -> [(u8, u32); REGISTERS] {
    params.serial.flips(index)
}

/// Converts the charge stream to volts at the ADC input for frame `index`.
///
/// Runs the analog chain described in the [module documentation](self) over every
/// stored sample and every blanking interval of the frame, and leaves the analog
/// state ready for the next frame. With a [`ResetGate`] each sample first passes the
/// floating diffusion of the module documentation's "Reset gate". Before the first
/// line the processor's serial writes of frame `index` program the registers (see
/// "Serial interface"), whose gains, pedestal and output controls apply to the frame.
///
/// `rail` is the supply rail of every readout line from
/// [`power::track`](crate::power::track), which sets the reference and the pedestal,
/// locks the chip out below [`Params::uvlo_v`] and restarts its clamp after that (see
/// the module documentation's "Supply").
///
/// # Panics
///
/// Panics if `out` does not have the layout of `stream`, or `rail` has another
/// number of lines.
pub fn process(
    params: &Params,
    state: &mut State,
    stream: &Stream,
    rail: &Rail,
    index: u64,
    out: &mut Analog,
) {
    let layout = stream.layout();
    assert!(
        out.layout == *layout,
        "analog layout {:?} does not match the stream layout {layout:?}",
        out.layout
    );
    assert_eq!(
        rail.lines(),
        layout.rows(),
        "the rail has {} lines but the stream {}",
        rail.lines(),
        layout.rows()
    );
    // In the vertical blanking before the frame the processor rewrites the registers,
    // which take effect at VD.
    state.serial = params.serial.program(state.serial, index);
    let setting = Setting::new(params, state.serial.as_ref());
    // Tristated outputs leave the bus holding the last word of the frame before.
    let held_v = setting
        .tristated
        .then(|| (state.last_v / params.code_v).round() * params.code_v);
    let mut front = Front::new(params, state, &setting, layout, rail, index);
    let line_s = layout.line_period_s();
    let hblank_s = f64::from(layout.hblank_px()) * layout.pixel_period_s();
    let line_len = layout.line_len() as usize;
    let hblank_px = u64::from(layout.hblank_px());
    let lines = out
        .volts
        .chunks_exact_mut(line_len)
        .zip(stream.samples().chunks_exact(line_len));
    let supply = rail.lines_v();
    for (line, (volts, electrons)) in (0..layout.rows()).zip(lines) {
        let start_s = f64::from(line) * line_s;
        let line_supply = supply[line as usize];
        let rows = (line, layout.image_row(line));
        front.line(rows, start_s, electrons, volts, line_supply);
        front.blank(hblank_s, start_s + line_s, line + 1, line_supply[1]);
        front.skip_resets(hblank_px);
    }
    let frame_lines = layout.rows() + layout.vblank_lines();
    let frame_s = params.frame_period_s;
    // The rail is not tracked through vertical blanking; it is taken where the last line left it.
    let end_v = supply.last().map_or(rail.nominal_v(), |line| line[1]);
    front.blank(vblank_s(params, layout), frame_s, frame_lines, end_v);
    front.skip_resets(vblank_periods(params, layout));
    if let Some((osc, _, start)) = front.inject {
        state.inject = osc.phase(start, frame_s, frame_lines);
    }
    if let Some((osc, _, start)) = front.gain_mod {
        state.gain = osc.phase(start, frame_s, frame_lines);
    }
    state.node_v = front.node_v;
    state.flicker_v = front.noise.flicker_v;
    state.rail_v = params.vref_v * (end_v / rail.nominal_v());
    state.last_v = front.power.last_v;
    if let Some(held_v) = held_v {
        // The analog chain ran on behind the outputs; the bus kept the word.
        out.volts.fill(held_v as f32);
        state.last_v = held_v;
    }
    state.locked = front.power.locked;
    state.restart_v = front.power.restart_v;
    if let Some(diffusion) = &front.diffusion {
        state.fd_charge_e = diffusion.charge_e;
        state.reset_missed = diffusion.resets.missed;
    } else {
        // A sound reset gate leaves the frame's last packet on the diffusion.
        state.fd_charge_e = stream.samples().last().map_or(0.0, |&e| f64::from(e));
        state.reset_missed = false;
    }
    out.index = index;
}
