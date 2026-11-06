//! The frame-buffer SDRAM: the raw frame stored and read back through its address pins.
//!
//! # Place in the chain
//!
//! Scene, sensor, readout, power, AFE, ADC, bus bend, capture, black clamp, then
//! **[`bend`]**, then development and codec. [`bend`] rewrites the clamped [`Raw`]
//! in place, between [`isp::clamp_black`](crate::isp::clamp_black) and
//! [`isp::develop`](crate::isp::develop), while the preset has a `[bend.memory]`
//! section; without one the raw passes untouched.
//!
//! # Addresses
//!
//! The processor's DMA writes the clamped raw into its SDRAM as the lines arrive, one
//! burst per line, and the image pipeline reads the frame back a frame period later to
//! develop it. Raw site `(x, y)`, rows in image order, lies at the linear word address
//!
//! > `a(x, y) = raw_base + y * pitch + x`, with `pitch = cols`,
//!
//! one sample per word. The real buffer holds packed 10-bit integers; the packing is
//! not modelled, and a word holds the clamped sample as an `f32` in processor codes
//! above black, so a sample read back into another row keeps the black level of the
//! row that wrote it.
//!
//! The memory controller splits the linear address into the SDRAM's column (the low
//! `column_bits`), bank (the next `bank_bits`) and row (the top `row_bits`): the
//! row-bank-column order that spreads a stream of bursts over the banks. This mapping
//! is a modelling choice; DIGIC's is undocumented. The SDRAM takes the row address
//! (with ACTIVE) and the column address (with READ or WRITE) on the same pins, so pin
//! `a_p` carries
//!
//! - column bit `p`, linear bit `c = p`, if `p < column_bits`, and
//! - row bit `p`, linear bit `r = column_bits + bank_bits + p`, if `p < row_bits`:
//!
//! one bad pin corrupts two bits of the linear address at once. The bendable pins are
//! `a0` to `a(max(column_bits, row_bits) - 1)`; the bank pins are not bendable.
//!
//! # Faults
//!
//! A fault acts on a linear address `a` through the bits its pin carries:
//!
//! - `low` clears both, `a & !(2^c | 2^r)`;
//! - `high` sets both, `a | 2^c | 2^r`;
//! - `bridge` with pin `q`: in each phase, column and row, both pins read the OR of
//!   their two bits (active-high dominance), so where either bit is set both are. A
//!   pin that carries no bit in a phase is driven low by the controller in it.
//!
//! A solid fault folds the frame onto itself in exact powers of two: addresses that
//! differ only in a faulted bit share one word, and every site that maps there reads
//! the sample written last. A fault that sends an address beyond the raw buffer puts
//! the sample in memory the frame does not otherwise use, and reads it back from there.
//!
//! The A520's pitch is no power of two (2272 is 32 times 71), so a weight `w` moves a sample
//! by `w / pitch` rows and `w mod pitch` columns, one row more where the columns wrap
//! past the line's end, and a column bit above the pitch's lowest set bit shifts its
//! fold from row to row. An odd row offset lands the copy on the other Bayer row
//! phase, where the green samples develop as red and blue and the red and blue as
//! green: a magenta or green cast. The movie raw holds a quarter of the rows, so a row
//! bit above its span (`a9` to `a11` on the A520) leaves a movie frame alone.
//!
//! # Marginal contacts
//!
//! A fault with `on_frac` and `switch_hz` is a marginal contact: a random telegraph
//! process in camera time, faulted for a fraction `π = on_frac` of the time, with
//! exponential holding times of mean `π / switch_hz` faulted and `(1 - π) / switch_hz`
//! clear. It is evaluated once per readout line, at the line's burst. With
//! `λ = switch_hz / (π (1 - π))`, a sample `Δ` seconds after one in state `s` is
//! faulted with probability
//!
//! > `P(on | on) = π + (1 - π) e^(-λΔ)`, `P(on | off) = π (1 - e^(-λΔ))`,
//!
//! the transition probabilities of that two-state process, so one uniform draw per
//! line samples it exactly at any switching rate. The draws come from a [`Pcg32`]
//! seeded from the stage seed and the pin, with the frame index as the stream id. A
//! contact that appears powers up faulted with probability `π`. `on_frac = 1` is the
//! solid fault; `switch_hz = 0` freezes the contact in its first state. The process
//! persists across frames in [`State`].
//!
//! # When the frame is written and read
//!
//! Readout line `l` (its image row follows from the readout direction) is written at
//! `t0 + l * line_period_s` through the faults as they are then, and read back at
//! `t0 + frame_period_s + l * line_period_s`, in readout order, through the faults as
//! they are then; the read replaces the raw. The next frame's writes start a frame
//! period after this frame's, so this frame's read instants are the next frame's write
//! instants: a contact's read samples are the next frame's write samples. Writes go in readout
//! order, line by line and column by column, so of two sites that share a word the
//! later-written one wins. Where a contact differs between a line's write and its
//! read, the read finds what the write did not put there: an earlier frame's sample
//! (the stale frames and torn bands of the Fujifilm S9000's documented frame-buffer
//! corruption, `docs/research/circuit-bend-reference.md`), another row of this frame,
//! or the power-on content.
//!
//! In the camera the next frame's writes interleave with this frame's reads; here they
//! follow them, so a read never sees the next frame's samples.
//!
//! A processor that browns out mid-frame captures no more lines
//! ([`Raw::dropped_from`]), so its DMA writes none of them: from the first dropped line
//! on, in readout order, no row is stored, its words keep what they held (the rows of
//! the last frame that reached them, or the power-on content), and the read-back shows
//! that where the black clamp left zero rows. Without `[bend.memory]` the stage does
//! not run and those rows stay black.
//!
//! # Memory content
//!
//! [`State`] holds the SDRAM's words over the span the frame can reach: the raw buffer
//! `[lo, hi)` and wherever the faults can send its addresses. A fault changes only
//! the faulted bits, all below `H = 2^(h + 1)` for the highest faulted bit `h`, so the
//! span `[lo & !(H - 1), (hi - 1) | (H - 1)]` covers every address. The store covers
//! every span needed since power-on and keeps its content across frames and across
//! [`Simulator::set_preset`](crate::Simulator::set_preset): new faults read the old
//! content. A word never written holds the power-on content, uniform random codes of
//! the processor's `dsp_bits` hashed from the address and the camera's `unit_seed`,
//! less the nominal black level: a cell's power-up state belongs to the chip, not the
//! preset (PROVISIONAL: the A520 SDRAM's power-up content is unknown).
//!
//! The store costs four bytes per word. At native resolution the still raw alone is
//! 2272 by 1704 words, 15.5 MB; a fault on the top row pin `a11` widens the span to
//! the whole 2^23-word SDRAM, 33.6 MB. A 640-wide preview needs a sixteenth of that.
//!
//! # Resolution
//!
//! At a virtual geometry of `cols` by `rows` raw sites against the native `cols_n` by
//! `rows_n` (the native rows read in the mode), every linear weight is scaled and
//! rounded to a power of two: a column bit `c` becomes `c + round(log2(cols / cols_n))`
//! and a row bit `r` becomes `r + round(log2(cols * rows / (cols_n * rows_n)))`, and
//! `raw_base` moves with the row bits. A fold then spans the same fraction of the
//! width or height as natively, exactly when the ratios are powers of two (568 by
//! 426 against 2272 by 1704). A column bit other than bit 0 stays at least bit 1, so a
//! fold keeps the Bayer column phase it has natively; at small sizes several native
//! bits land on one, and the faults then apply in the preset's order. The pitch is
//! not a power of two, so a fold's row offset, and with it the Bayer row phase of the
//! folded copy, can differ between a virtual geometry and native resolution.
//!
//! Not modelled: the SDRAM's burst counter (every word's column address passes the
//! pins), the auto-precharge role of `a10` in column commands, refresh, and the
//! interleaving of the next frame's writes with this frame's reads.

use std::fmt::{self, Debug, Formatter};
use std::ops::Range;

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::isp::Raw;
use crate::noise::{self, Pcg32};
use crate::preset::{AddressFault, FaultKind, Mode};
use crate::readout::Direction;
use crate::{Camera, Error, Preset, Timing};

/// Salt of the stage seed, from which the contacts' streams derive.
const SALT: u32 = noise::salt("memory");

/// Salt of each marginal contact's stream, combined with its pin.
const SALT_CONTACT: u32 = noise::salt("memory.contact");

/// Salt of the SDRAM's power-on content, hashed with the camera's `unit_seed`.
const SALT_POWER_ON: u32 = noise::salt("memory.power-on");

/// Scale that maps 32 random bits onto `[0, 1)`.
const UNIT_32: f64 = 1.0 / 4_294_967_296.0;

/// Most faults the per-line masks hold; each fault needs its own pin, and a camera has at most 16.
const MAX_FAULTS: usize = 32;

/// A marginal contact: a random telegraph process in camera time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// Fraction of the time the contact is faulted, in `(0, 1)`.
    pub on_frac: f64,
    /// Mean on-off cycles per second, at least 0.
    pub switch_hz: f64,
}

impl Contact {
    /// Returns the probability that the contact is faulted `dt_s` after a sample in state `on`.
    ///
    /// `π + (1 - π) e^(-λ dt)` from faulted, `π (1 - e^(-λ dt))` from clear, with
    /// `π = on_frac` and `λ = switch_hz / (π (1 - π))`. A contact faulted all the time
    /// (`on_frac` 1) stays faulted.
    #[must_use]
    pub fn on_probability(&self, on: bool, dt_s: f64) -> f64 {
        let pi = self.on_frac;
        if pi >= 1.0 {
            return 1.0;
        }
        let rate = self.switch_hz / (pi * (1.0 - pi));
        let memory = (-rate * dt_s.max(0.0)).exp();
        if on {
            pi + (1.0 - pi) * memory
        } else {
            pi * (1.0 - memory)
        }
    }
}

/// One faulted address pin with the linear address bits it drives at the simulated geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    /// The faulted pin, `a0` upward.
    pub pin: u8,
    /// What the fault does.
    pub kind: FaultKind,
    /// The other pin of a bridge.
    pub with: Option<u8>,
    /// The linear address bits of the pin's column and row address; `None` where it carries none.
    pub bits: [Option<u32>; 2],
    /// The bits of the bridge's other pin, likewise; both `None` for a stuck pin.
    pub with_bits: [Option<u32>; 2],
    /// The marginal contact; `None` for a solid fault.
    pub contact: Option<Contact>,
    /// The address operation, from the bits.
    action: Action,
}

/// What a fault does to a linear address, as bit masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Clears the bits.
    Clear(u64),
    /// Sets the bits.
    Set(u64),
    /// In each mask, sets both bits where either is set.
    Or([u64; 2]),
}

impl Fault {
    /// Resolves one preset fault with the bit shifts of the geometry.
    fn new(fault: &AddressFault, layout: &BitLayout) -> Self {
        let bits = layout.pin_bits(fault.pin);
        let with_bits = fault.with.map_or([None; 2], |pin| layout.pin_bits(pin));
        let mask = |bit: Option<u32>| bit.map_or(0, |b| 1u64 << b);
        let action = match fault.kind {
            FaultKind::Low => Action::Clear(mask(bits[0]) | mask(bits[1])),
            FaultKind::High => Action::Set(mask(bits[0]) | mask(bits[1])),
            FaultKind::Bridge => Action::Or([
                mask(bits[0]) | mask(with_bits[0]),
                mask(bits[1]) | mask(with_bits[1]),
            ]),
        };
        let contact = match (fault.on_frac, fault.switch_hz) {
            (Some(on_frac), Some(switch_hz)) if on_frac < 1.0 => {
                Some(Contact { on_frac, switch_hz })
            }
            _ => None,
        };
        Self {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            bits,
            with_bits,
            contact,
            action,
        }
    }

    /// Returns the address the SDRAM decodes for linear address `address` while the fault acts.
    #[must_use]
    pub fn apply(&self, address: u64) -> u64 {
        match self.action {
            Action::Clear(mask) => address & !mask,
            Action::Set(mask) => address | mask,
            Action::Or(masks) => masks
                .iter()
                .fold(address, |a, &mask| if a & mask == 0 { a } else { a | mask }),
        }
    }

    /// Returns every linear address bit the fault can change.
    fn mask(&self) -> u64 {
        match self.action {
            Action::Clear(mask) | Action::Set(mask) => mask,
            Action::Or([column, row]) => column | row,
        }
    }

    /// Returns whether `contact` sits on this fault's wiring: the same pins and kind.
    fn same_wiring(&self, contact: &ContactState) -> bool {
        self.pin == contact.pin && self.kind == contact.kind && self.with == contact.with
    }
}

/// Where the pins' bits lie in the linear address at the simulated geometry.
struct BitLayout {
    column_bits: u32,
    row_bits: u32,
    /// Linear bit of row bit 0 at native resolution: `column_bits + bank_bits`.
    row_base: u32,
    /// Shift of the column bits, `round(log2(cols / cols_n))`.
    column_shift: i32,
    /// Shift of the row bits, `round(log2(cols * rows / (cols_n * rows_n)))`.
    row_shift: i32,
}

impl BitLayout {
    /// Returns the column and row bit pin `pin` carries, scaled to the geometry.
    fn pin_bits(&self, pin: u8) -> [Option<u32>; 2] {
        let pin = u32::from(pin);
        // Bit 0 stays bit 0 and every other bit stays at least bit 1, so a fold keeps
        // the Bayer column phase it has natively.
        let scaled = |native: u32, shift: i32| {
            let floor = native.min(1);
            (i64::from(native) + i64::from(shift)).clamp(i64::from(floor), 62) as u32
        };
        [
            (pin < self.column_bits).then(|| scaled(pin, self.column_shift)),
            (pin < self.row_bits).then(|| scaled(self.row_base + pin, self.row_shift)),
        ]
    }
}

/// Returns `round(log2(ratio))`.
fn octaves(ratio: f64) -> i32 {
    ratio.log2().round() as i32
}

/// The SDRAM's power-on content: hashed codes, fixed for the unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerOn {
    /// Seed of the hash, from the camera's `unit_seed`.
    pub seed: u64,
    /// Largest code the processor reads, `2^dsp_bits - 1`.
    pub max_code: u32,
    /// Nominal black level in processor codes, subtracted as the clamp would.
    pub black_code: f32,
}

impl PowerOn {
    /// Returns the word at `address` before anything was written there, in codes above black.
    #[must_use]
    pub fn word(&self, address: u64) -> f32 {
        let hash = noise::hash_u32(
            address as u32,
            (address >> 32) as u32,
            SALT_POWER_ON,
            self.seed,
        );
        let code = hash % (self.max_code + 1);
        code as f32 - self.black_code
    }
}

/// The resolved address faults and the frame buffer's layout, present with `[bend.memory]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Bits of the SDRAM's linear word address: column, row and bank bits together.
    pub address_bits: u32,
    /// Bendable address pins, `a0` to `a(pins - 1)`: `max(column_bits, row_bits)`.
    pub pins: u32,
    /// Word address of the raw buffer's first sample at this geometry.
    pub raw_base: u64,
    /// Words from one raw row to the next: the active columns.
    pub pitch: u32,
    /// Raw rows, one per readout line.
    pub rows: u32,
    /// Which end of the sensor is read, and so written, first.
    pub direction: Direction,
    /// Time from one line's burst to the next line's, in seconds.
    pub line_period_s: f64,
    /// Time from a line's write to its read, one frame period, in seconds.
    pub frame_period_s: f64,
    /// The faulted pins in the preset's order, with their bits at this geometry.
    pub faults: Vec<Fault>,
    /// The word addresses the frame can reach: the raw buffer and every faulted address.
    pub span: Range<u64>,
    /// Seed of the contacts' streams.
    pub seed: u64,
    /// What a word holds before it is first written.
    pub power_on: PowerOn,
}

impl Params {
    /// Resolves the faults against the camera's `[memory]` section; `None` without `[bend.memory]`.
    ///
    /// The bits each pin drives are scaled from the native readout to the geometry of
    /// `timing` as the [module documentation](self) describes.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a fault is invalid on its own, names a pin
    /// beyond `a(max(column_bits, row_bits) - 1)` (the bank pins are not bendable), or
    /// if the native raw buffer does not fit in the SDRAM above `raw_base`.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let Some(bend) = &preset.bend.memory else {
            return Ok(None);
        };
        bend.validate()?;
        let memory = &camera.memory;
        let address_bits = memory.address_bits();
        let pins = u32::from(memory.column_bits.max(memory.row_bits));
        for (index, fault) in bend.faults.iter().enumerate() {
            for (key, pin) in [("pin", Some(fault.pin)), ("with", fault.with)] {
                if let Some(pin) = pin.filter(|&pin| u32::from(pin) >= pins) {
                    return Err(Error::config(format!(
                        "bend.memory.faults[{index}].{key} names pin a{pin}, but the camera's \
                         SDRAM multiplexes its row and column addresses on pins a0 to a{}; \
                         the bank pins are not bendable",
                        pins - 1
                    )));
                }
            }
        }
        debug_assert!(
            bend.faults.len() <= MAX_FAULTS,
            "{} faults on {pins} pins passed validation",
            bend.faults.len()
        );

        let line_skip = match timing.mode {
            Mode::Still => 1,
            Mode::Movie => camera.movie.line_skip.max(1),
        };
        let native_cols = u64::from(camera.sensor.native_width);
        let native_rows = u64::from(camera.sensor.native_height / (2 * line_skip) * 2);
        let native_words = native_cols * native_rows;
        if memory.raw_base.saturating_add(native_words) > 1u64 << address_bits {
            return Err(Error::config(format!(
                "the native raw buffer of {native_words} words at memory.raw_base {} does \
                 not fit in the SDRAM's {} words",
                memory.raw_base,
                1u64 << address_bits
            )));
        }

        let (cols, rows) = (timing.cols, timing.rows_read);
        let column_shift = octaves(f64::from(cols) / native_cols as f64);
        let row_shift =
            octaves(f64::from(cols) * f64::from(rows) / (native_cols as f64 * native_rows as f64));
        let layout = BitLayout {
            column_bits: u32::from(memory.column_bits),
            row_bits: u32::from(memory.row_bits),
            row_base: u32::from(memory.column_bits) + u32::from(memory.bank_bits),
            column_shift,
            row_shift,
        };
        let raw_base = if row_shift >= 0 {
            memory.raw_base << row_shift.unsigned_abs()
        } else {
            memory.raw_base >> row_shift.unsigned_abs()
        };
        let faults: Vec<Fault> = bend
            .faults
            .iter()
            .map(|fault| Fault::new(fault, &layout))
            .collect();

        let end = raw_base + u64::from(cols) * u64::from(rows);
        let faulted = faults.iter().fold(0, |mask, fault| mask | fault.mask());
        let below = match faulted {
            0 => 0,
            mask => u64::MAX >> mask.leading_zeros(),
        };
        let span = (raw_base & !below)..((end - 1) | below) + 1;

        let dsp_bits = u32::from(camera.adc.dsp_bits.min(16));
        Ok(Some(Self {
            address_bits,
            pins,
            raw_base,
            pitch: cols,
            rows,
            direction: camera.clocking(timing.mode).direction,
            line_period_s: timing.line_period_s,
            frame_period_s: timing.frame_period_s,
            faults,
            span,
            seed: noise::stage_seed(preset.seed, SALT),
            power_on: PowerOn {
                seed: noise::stage_seed(camera.sensor.unit_seed, SALT_POWER_ON),
                max_code: (1u32 << dsp_bits) - 1,
                black_code: f32::from(camera.adc.black_level >> camera.adc.dsp_lsb),
            },
        }))
    }

    /// Returns the linear word address of raw site `(x, y)`, rows in image order.
    #[must_use]
    pub fn address(&self, x: u32, y: u32) -> u64 {
        self.raw_base + u64::from(y) * u64::from(self.pitch) + u64::from(x)
    }

    /// Returns the readout line that carries image row `y`; the map is its own inverse.
    #[must_use]
    pub fn line(&self, y: u32) -> u32 {
        match self.direction {
            Direction::TopDown => y,
            Direction::BottomUp => self.rows - 1 - y,
        }
    }

    /// Returns the address the SDRAM decodes for `address` while the faults in `active` act.
    ///
    /// Bit `i` of `active` stands for `faults[i]`; the faults apply in order.
    #[must_use]
    pub fn decode(&self, address: u64, active: u32) -> u64 {
        Decoder::new(&self.faults, active).decode(address)
    }

    /// Returns the seed of the stream of the contact on `pin`.
    fn contact_seed(&self, pin: u8) -> u64 {
        noise::stage_seed(self.seed, SALT_CONTACT ^ u32::from(pin))
    }
}

/// One step of a [`Decoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// `(a & keep) | set`: any run of stuck pins, composed.
    Mask { keep: u64, set: u64 },
    /// A bridge: in each mask, both bits where either is set.
    Or([u64; 2]),
}

/// The faults acting on one burst, composed into as few steps as their order allows.
#[derive(Debug, Clone, Copy)]
struct Decoder {
    steps: [Step; MAX_FAULTS],
    len: usize,
}

impl Decoder {
    /// Composes the faults whose bit is set in `active`, in order.
    fn new(faults: &[Fault], active: u32) -> Self {
        let mut decoder = Self {
            steps: [Step::Or([0; 2]); MAX_FAULTS],
            len: 0,
        };
        for (i, fault) in faults.iter().enumerate() {
            if active >> i & 1 == 0 {
                continue;
            }
            let (keep, set) = match fault.action {
                Action::Clear(mask) => (!mask, 0),
                Action::Set(mask) => (u64::MAX, mask),
                Action::Or(masks) => {
                    decoder.steps[decoder.len] = Step::Or(masks);
                    decoder.len += 1;
                    continue;
                }
            };
            // ((a & k0) | s0) & k | s = (a & k0 & k) | (s0 & k | s)
            let last = decoder.len.checked_sub(1);
            if let Some(Step::Mask { keep: k0, set: s0 }) =
                last.map(|last| &mut decoder.steps[last])
            {
                *k0 &= keep;
                *s0 = (*s0 & keep) | set;
            } else {
                decoder.steps[decoder.len] = Step::Mask { keep, set };
                decoder.len += 1;
            }
        }
        decoder
    }

    /// Returns whether no fault acts.
    fn is_identity(&self) -> bool {
        self.len == 0
    }

    /// Returns the address the SDRAM decodes for `address`.
    fn decode(&self, address: u64) -> u64 {
        self.steps[..self.len]
            .iter()
            .fold(address, |a, step| match *step {
                Step::Mask { keep, set } => (a & keep) | set,
                Step::Or(masks) => masks
                    .iter()
                    .fold(a, |a, &mask| if a & mask == 0 { a } else { a | mask }),
            })
    }
}

/// The SDRAM's words over the addresses the frames have reached.
#[derive(Clone, Default, PartialEq)]
struct Sdram {
    /// Address of `words[0]`.
    start: u64,
    words: Vec<f32>,
}

impl Sdram {
    /// Returns the addresses the store covers.
    fn span(&self) -> Range<u64> {
        self.start..self.start + self.words.len() as u64
    }

    /// Widens the store to cover `span`, filling new words with the power-on content.
    fn cover(&mut self, span: &Range<u64>, power_on: &PowerOn) {
        let have = self.span();
        if !self.words.is_empty() && have.start <= span.start && span.end <= have.end {
            return;
        }
        let new = if self.words.is_empty() {
            span.clone()
        } else {
            have.start.min(span.start)..have.end.max(span.end)
        };
        let len = usize::try_from(new.end - new.start).expect("the SDRAM span fits in memory");
        let mut words = vec![0.0; len];
        let start = new.start;
        words
            .par_chunks_mut(4096)
            .enumerate()
            .for_each(|(chunk, slots)| {
                let first = start + (chunk * 4096) as u64;
                for (offset, slot) in (0u64..).zip(slots.iter_mut()) {
                    *slot = power_on.word(first + offset);
                }
            });
        if !self.words.is_empty() {
            let at = (have.start - start) as usize;
            words[at..at + self.words.len()].copy_from_slice(&self.words);
        }
        self.start = start;
        self.words = words;
    }

    /// Returns the index of `address` in `words`.
    fn slot(&self, address: u64) -> usize {
        (address - self.start) as usize
    }
}

/// One marginal contact's process, sampled at every readout line.
#[derive(Debug, Clone, PartialEq)]
struct ContactState {
    pin: u8,
    kind: FaultKind,
    with: Option<u8>,
    /// Whether the contact was faulted at each readout line's write of the last frame.
    write: Vec<bool>,
    /// Whether it was faulted at each line's read, the next frame's write.
    read: Vec<bool>,
}

/// The SDRAM's memory from frame to frame; the default is the power-on state.
///
/// Holds the content of every word the frames have reached and the marginal contacts'
/// states. A store is allocated with the first frame and widened when a new fault
/// reaches further, never in the steady state.
#[derive(Clone, Default, PartialEq)]
pub struct State {
    sdram: Sdram,
    contacts: Vec<ContactState>,
    /// The faults acting at each readout line's write, bit `i` for fault `i`.
    write_active: Vec<u32>,
    /// The faults acting at each readout line's read.
    read_active: Vec<u32>,
}

impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("span", &self.sdram.span())
            .field("contacts", &self.contacts.len())
            .finish_non_exhaustive()
    }
}

impl State {
    /// Returns the word at `address`, or `None` outside the store, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn word(&self, address: u64) -> Option<f32> {
        self.sdram
            .span()
            .contains(&address)
            .then(|| self.sdram.words[self.sdram.slot(address)])
    }

    /// Returns the addresses the store covers, empty before the first frame, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn span(&self) -> Range<u64> {
        self.sdram.span()
    }

    /// Returns whether the contact on `pin` was faulted at each line's write and read of the last frame, for tests.
    ///
    /// Indexed by readout line; `None` if no marginal contact sits on `pin`.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn contact_lines(&self, pin: u8) -> Option<(&[bool], &[bool])> {
        self.contacts
            .iter()
            .find(|contact| contact.pin == pin)
            .map(|contact| (contact.write.as_slice(), contact.read.as_slice()))
    }

    /// Samples every marginal contact over this frame's writes and reads.
    ///
    /// A contact that sat on the same wiring last frame writes with its read samples
    /// and continues from its last one; a new contact powers up at the first write.
    fn sample_contacts(&mut self, params: &Params, index: u64) {
        let rows = params.rows as usize;
        self.contacts.retain(|contact| {
            contact.read.len() == rows
                && params
                    .faults
                    .iter()
                    .any(|fault| fault.contact.is_some() && fault.same_wiring(contact))
        });
        let gap_s = params.frame_period_s - params.line_period_s * f64::from(params.rows - 1);
        for fault in &params.faults {
            let Some(contact) = fault.contact else {
                continue;
            };
            let mut rng = Pcg32::new(params.contact_seed(fault.pin), index);
            let at = if let Some(at) = self
                .contacts
                .iter()
                .position(|state| fault.same_wiring(state))
            {
                let state = &mut self.contacts[at];
                std::mem::swap(&mut state.write, &mut state.read);
                at
            } else {
                let mut write = Vec::with_capacity(rows);
                let mut on = chance(&mut rng, contact.on_frac);
                write.push(on);
                for _ in 1..rows {
                    on = chance(&mut rng, contact.on_probability(on, params.line_period_s));
                    write.push(on);
                }
                self.contacts.push(ContactState {
                    pin: fault.pin,
                    kind: fault.kind,
                    with: fault.with,
                    write,
                    read: vec![false; rows],
                });
                self.contacts.len() - 1
            };
            let state = &mut self.contacts[at];
            let mut on = state.write[rows - 1];
            for (line, slot) in state.read.iter_mut().enumerate() {
                let dt_s = if line == 0 {
                    gap_s
                } else {
                    params.line_period_s
                };
                on = chance(&mut rng, contact.on_probability(on, dt_s));
                *slot = on;
            }
        }

        self.write_active.resize(rows, 0);
        self.read_active.resize(rows, 0);
        self.write_active.fill(0);
        self.read_active.fill(0);
        for (i, fault) in params.faults.iter().enumerate() {
            let bit = 1u32 << i;
            let contact = self
                .contacts
                .iter()
                .find(|state| fault.contact.is_some() && fault.same_wiring(state));
            for line in 0..rows {
                let (write, read) =
                    contact.map_or((true, true), |state| (state.write[line], state.read[line]));
                if write {
                    self.write_active[line] |= bit;
                }
                if read {
                    self.read_active[line] |= bit;
                }
            }
        }
    }
}

/// Returns `true` with probability `probability`, from one draw of `rng`.
fn chance(rng: &mut Pcg32, probability: f64) -> bool {
    f64::from(rng.next_u32()) * UNIT_32 < probability
}

/// Rewrites `raw` with frame `index` as the processor reads it back from the SDRAM.
///
/// Without a bend (`params` is `None`) it does nothing. With one, every image row the
/// processor captured is stored in readout order, one row at a time, through the faults
/// acting at its write, then the whole frame is read back a frame period later through
/// the faults acting then; see the [module documentation](self). The rows from
/// [`Raw::dropped_from`] on, in readout order, are not stored: their read finds the
/// SDRAM's earlier content. Without faults or a brownout the raw comes back unchanged.
///
/// # Panics
///
/// Panics if `raw` is not `pitch` by `rows` sites of `params`.
pub fn bend(params: Option<&Params>, state: &mut State, index: u64, raw: &mut Raw) {
    let Some(params) = params else {
        return;
    };
    let plane = raw.plane();
    assert!(
        plane.width() == params.pitch && plane.height() == params.rows,
        "raw plane is {} x {} but the frame buffer holds {} x {}",
        plane.width(),
        plane.height(),
        params.pitch,
        params.rows
    );
    state.sdram.cover(&params.span, &params.power_on);
    state.sample_contacts(params, index);

    // From the first line the processor dropped on, nothing reaches the SDRAM.
    let captured = raw.dropped_from().map_or(params.rows, |y| params.line(y));
    let mut in_place = 0;
    for line in 0..captured {
        let y = params.line(line);
        let active = state.write_active[line as usize];
        if store_row(params, state, y, active, raw.plane().row(y)) {
            in_place += 1;
        }
    }
    // When every row went to its own words, a row read through no fault reads itself;
    // a row that was never stored reads what its words hold.
    let reread = in_place < params.rows;

    let (sdram, read_active) = (&state.sdram, &state.read_active);
    raw.plane_mut()
        .data_mut()
        .par_chunks_mut(params.pitch as usize)
        .enumerate()
        .for_each(|(y, sites)| {
            let y = y as u32;
            let active = read_active[params.line(y) as usize];
            load_row(params, sdram, y, active, reread, sites);
        });
}

/// Writes image row `y` into the SDRAM through the faults in `active`, site by site in column order.
///
/// Every captured row of a frame is stored through this function, in readout order.
/// Returns whether the row went to its own words.
fn store_row(params: &Params, state: &mut State, y: u32, active: u32, sites: &[f32]) -> bool {
    let sdram = &mut state.sdram;
    let first = params.address(0, y);
    let decoder = Decoder::new(&params.faults, active);
    if decoder.is_identity() {
        let at = sdram.slot(first);
        sdram.words[at..at + sites.len()].copy_from_slice(sites);
        return true;
    }
    for (x, &value) in (0u64..).zip(sites) {
        let at = sdram.slot(decoder.decode(first + x));
        sdram.words[at] = value;
    }
    false
}

/// Reads image row `y` from the SDRAM through the faults in `active`.
///
/// A row read through no fault is read only if `reread`: otherwise it holds what it stored.
fn load_row(params: &Params, sdram: &Sdram, y: u32, active: u32, reread: bool, sites: &mut [f32]) {
    let first = params.address(0, y);
    let decoder = Decoder::new(&params.faults, active);
    if decoder.is_identity() {
        if reread {
            let at = sdram.slot(first);
            sites.copy_from_slice(&sdram.words[at..at + sites.len()]);
        }
        return;
    }
    for (x, site) in (0u64..).zip(sites.iter_mut()) {
        *site = sdram.words[sdram.slot(decoder.decode(first + x))];
    }
}
