//! The frame-buffer SDRAM: the raw and the JPEG scan read back through its pins and cells.
//!
//! # Place in the chain
//!
//! Scene, sensor, readout, power, AFE, ADC, bus bend, capture, black clamp, then
//! **[`bend`]**, then development and codec. [`bend`] rewrites the clamped [`Raw`]
//! in place, between [`isp::clamp_black`](crate::isp::clamp_black) and
//! [`isp::develop`](crate::isp::develop), while the preset has a `[bend.memory]`
//! section; without one the raw passes untouched.
//!
//! The compressed frame can pass the same SDRAM: **[`bend_scan`]** rewrites the codec's
//! entropy-coded scan in place, between
//! [`codec::encode_scan`](crate::codec::encode_scan) and
//! [`codec::decode_scan`](crate::codec::decode_scan), through the same pins and cells
//! ([The JPEG buffer](#the-jpeg-buffer)), while the section sets `reaches_jpeg`
//! ([`Params::reaches_jpeg`]); otherwise the codec's direct round trip,
//! [`codec::encode`](crate::codec::encode), runs and the scan is out of the faults'
//! reach.
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
//! # Data pins
//!
//! A word holds the clamped sample as the integer the DMA wrote: the processor's
//! `dsp_bits`-bit code, the sample `v` with its row's black level `b` added back and
//! rounded,
//!
//! > `W = round(v + b)`, held within `0..2^dsp_bits`,
//!
//! in the word's low bits. Data pin `d_n` carries bit `n` of every word. The pins at
//! or above `dsp_bits` (`d10` to `d15` of the A520's 16) carry no sample bit: the DMA
//! drives them low and the processor ignores them (the packed layout that would fill
//! them is not modelled), so a fault on one is accepted and changes no sample, and a
//! bridge to one leaves its partner as it is (the OR with a bit that is never set);
//! the compressed scan fills them ([The JPEG buffer](#the-jpeg-buffer)). A fault on
//! pin `n` acts on the word's bits:
//!
//! - `low` clears bit `n`, `W & !2^n`;
//! - `high` sets it, `W | 2^n`;
//! - `bridge` with pin `m`: both read the OR of the two bits, so where either is set
//!   both are.
//!
//! Each pin carries one fault, so the faults act on disjoint bits and commute.
//!
//! A fault acts at the write: the stored word takes the faulted bits, `W' = f(W)`, and
//! the `f32` word holds `W' - b`, the faulted code less the writing row's black. It
//! acts again at the read, through the contact as it then is: the read reconstructs
//! the word as `W = round(v + b_r)` with the reading row's black `b_r`, the faults turn
//! it into `W''`, and the value read is `v + W'' - W`. The read thus changes a word by
//! the weights of the bits it changes and the word keeps the black of the row that
//! wrote it; the reconstruction is exact while the two rows' blacks agree within half
//! a code, as they do along a frame. A word whose bits a fault leaves alone keeps its
//! value bit for bit. The black clamp has run before the store, so a stuck bit shifts
//! the black with the signal: `d9` high puts every sample's code in the top half, at
//! 512 and above; `d9` low folds the highlights down by 512 codes; `d5` high lifts the
//! A520's black of 31 codes by 32.
//!
//! A solid fault reads stuck whatever was stored, decayed cells included: the word
//! crosses the pins after its cells are sensed. A marginal data contact is sampled as
//! an address contact is, once at each readout line's write burst and once at its
//! read burst ([Marginal contacts](#marginal-contacts)). It leaves a corrupted bit in
//! the word where it was faulted at the write, until the next write of the word, so a
//! clean read of a stale word, one that a marginal address contact or a brownout
//! reads back from an earlier frame, shows the bit of the write that left it there.
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
//! seeded from the stage seed and the pin, with the frame index as the stream id; a
//! data pin's seed takes a salt of its own, so contacts on `d3` and on `a3` draw
//! different streams. A contact that appears powers up faulted with probability `π`.
//! `on_frac = 1` is the solid fault; `switch_hz = 0` freezes the contact in its first
//! state. The process persists across frames in [`State`].
//!
//! # When the frame is written and read
//!
//! Readout line `l` (its image row follows from the readout direction) is written at
//! `t0 + l * line_period_s` through the address and data faults as they are then, and
//! read back at `t0 + frame_period_s + l * line_period_s`, in readout order, through
//! the cells and the faults as they are then; the read replaces the raw. Every
//! contact, address or data, is sampled at those two instants of each line. The next
//! frame's writes start a frame period after this frame's, so this frame's read
//! instants are the next frame's write instants: a contact's read samples are the next
//! frame's write samples. Writes go in readout order, line by line and column by
//! column, so of two sites that share a word the later-written one wins. Where a
//! contact differs between a line's write and its read, the read finds what the write
//! did not put there: an earlier frame's sample (the stale frames and torn bands of
//! the Fujifilm S9000's documented frame-buffer corruption,
//! `docs/research/circuit-bend-reference.md`), another row of this frame, or the
//! power-on content.
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
//! # Retention
//!
//! A DRAM cell holds its bit as charge that leaks away, and the controller refreshes
//! every row every `refresh_ms`, `t_ref`. The cell of bit `i` of the word at linear
//! address `a` keeps its charge for
//!
//! > `τ = retention_s * 2^(-(T - T_0) / retention_halving_c) * e^(retention_spread * N)`,
//!
//! `T` the SDRAM's `temperature_c`, `T_0` the camera's `reference_temp_c`, and `N` a
//! unit normal hashed from `a`, `i` and the camera's `unit_seed`: the chip's own map
//! of weak cells, the same in every frame and every preset. A cell with `τ < t_ref`
//! loses its bit before the refresh comes, and the refresh latches what it finds, so
//! at every read the cell reads its leakage state, whatever and whenever the write
//! was; a cell with `τ ≥ t_ref` is refreshed in time, every time, and never decays.
//! True cells, those of an even SDRAM row (the row of a word is its linear address's
//! row bits, so the parity is linear bit `r_0` of row bit 0 at the geometry), fall to
//! 0; anti cells, of an odd row, rise to 1 (PROVISIONAL: the folded bit-line layout
//! alternates them). The decayed cells of a word all fall or all rise, so a hot board
//! shows dark single sites in some SDRAM rows and bright ones in the next, each by the
//! weight of its bit.
//!
//! With `τ_m = retention_s * 2^(-(T - T_0) / retention_halving_c)` the median at `T`,
//! a cell decays when `N < n_c = ln(t_ref / τ_m) / retention_spread`, which a fraction
//! `p = Φ(n_c)` of the cells do (without spread, all cells or none). The normal is
//! drawn as `N = Φ⁻¹(U)`, `U = (h + 1/2) / 2^32` from a 32-bit hash `h` of the cell,
//! so the comparison is made on the hash: a cell decays exactly when
//! `h < ceil(p 2^32 - 1/2)`, at the cost of one hash per cell and no inverse. The
//! hashed tail ends at `Φ⁻¹(2^-33)`, 6.3 standard deviations. A raw sample reads the
//! cells of its sample bits only; a word of the scan, the cells of all its bits.
//!
//! On the A520 (PROVISIONAL: a median of 128 s at 25 °C, a spread of 0.5) the refresh
//! period lies 15 standard deviations below the median at the reference temperature,
//! far beyond the hashed tail: no cell decays and the default renders the camera's
//! bytes. Of a native still's sites, a decayed sample bit hits about 30 at 100 °C,
//! 0.3 % at 110 °C, a fifth at 120 °C and 95 % at 130 °C; a site shows it where the
//! stored bit differs from the leakage state.
//!
//! The stage keeps, per word of the raw's store, the masks of the sample bits that fall
//! and that rise at the current temperature, two 16-bit masks, four bytes a word, as
//! many as the store's: another 15.5 MB beside the native still raw; and per word of
//! the scan's store the masks of all 16 bits ([Memory content](#memory-content)). They
//! are computed, one hash per mapped bit of every word, when the cells that decay change
//! (a new temperature) and for the words a wider store adds, and kept otherwise; while
//! no cell can decay they are neither computed nor held. Every word the stage reads,
//! stale words included, passes them, `(W & !fall) | rise`, before the read-side data
//! faults act: the cells are sensed first, then the word crosses the pins. A word of
//! the scan in the raw's store takes its sample bits' masks from there and hashes the
//! cells of its high bits at the read.
//!
//! Not modelled: the age of a word beyond the refresh period (a cell decays or not by
//! its retention against the refresh period alone, so a word read a frame after its
//! write loses the same bits as one stale for a thousand frames, and a cell that
//! decayed reads its stored bit again once the board cools, where the chip keeps the
//! lost bit until the next write), the sliver between the frame period and the
//! refresh period (a cell that could outlast the gap to the read if the read came
//! before the refresh it misses decays all the same), and a row's refresh by its own
//! reads.
//!
//! # The JPEG buffer
//!
//! The encoder's DMA writes the compressed frame, the entropy-coded scan of
//! [`codec::encode_scan`](crate::codec::encode_scan), into the same SDRAM, and the card
//! writer reads it back from there; [`bend_scan`] puts the scan through both. Bytes
//! `2i` and `2i + 1` of the scan are the low and the high byte of word `i`
//! (PROVISIONAL: the DMA's byte order is unpublished), at the linear word address
//!
//! > `a_i = jpeg_base + i`,
//!
//! the camera's `[memory] jpeg_base` moved with the row bits as `raw_base` is
//! ([Resolution](#resolution)). A scan of odd length pads its last word with a zero
//! byte, which the read drops; the scan keeps its length. On the A520 `jpeg_base` is
//! 2^22 (PROVISIONAL), the weight of `a11`'s row bit, just above the native still raw,
//! which ends at 3,871,488 words.
//!
//! Whether the compressed buffer lies in the bent chip at all is the memory map's doing,
//! and unknown: the Fujifilm S9000's documented bend produced readable files with torn
//! raws, so its compressed buffer was out of the faults' reach. `[bend.memory]
//! reaches_jpeg` decides it (PROVISIONAL default `false`); without it the simulator
//! leaves the scan alone and runs the codec's direct round trip.
//!
//! The DMA writes the scan in stream order, in bursts of one readout line's worth of
//! words (`pitch`), after the frame's development, which follows the raw's read-back,
//! and the card writer reads it in the same bursts a frame period later (PROVISIONAL
//! timing): burst `k` is written at `t0 + frame_period_s + k * line_period_s` and read
//! at `t0 + 2 * frame_period_s + k * line_period_s`, `t0` the instant of the raw's
//! first line write. Both go by the raw's rules: the write stores through the address
//! and data faults as the contacts then are, and the later write wins where words
//! fold; the read finds what the faults then address, through the cells and then the
//! data faults as they then are. A word the scan did not write holds what it held: the
//! power-on content, a stale frame's scan, or the raw's samples.
//!
//! The scan fills the whole word, so all 16 data pins carry one of its bits and none
//! is inert: a fault on `d_n` acts on bit `n mod 8` of byte `2i + n / 8` of every word
//! `i`, one bit of every other byte (`d3` on bit 3 of every low byte, `d11` on bit 3
//! of every high byte). At the read the cells of all 16 bits decay, by the masks of
//! [Retention](#retention).
//!
//! **Contacts.** Burst `k`'s write shares its instant with the raw's read of readout
//! line `k`, and its read with the next frame's raw read of line `k`. A contact has one
//! state at an instant, so the write takes the raw's read sample of its line, and the
//! read the next frame's, which the stage draws ahead from the next frame's stream
//! (frame `index + 1`) exactly as that frame's [`bend`] will. The contacts run one path
//! through both buffers, and the raw's samples are the same whether or not the scan is
//! bent. A scan longer than the frame's lines has bursts at instants that no line
//! shares; each continues the path from the latest sample before it, in time order, on
//! a stream of the scan's own. Two things break the sharing: a next frame that is not
//! `index + 1`, or that changes the contact, draws reads of its own, so the scan's
//! reads are a path the process could have taken rather than the one the raw takes;
//! and a contact the frame's raw has not sampled (the scan bent without [`bend`])
//! powers up at the scan's first write.
//!
//! The stage runs a frame in the order raw writes, raw reads, scan writes, scan reads,
//! then the next frame's raw writes. In the camera the scan's writes interleave with
//! the raw's reads and the next frame's writes, and its reads with the next frame's
//! writes and reads; here a scan read never finds the next frame's raw, nor a raw read
//! this frame's scan.
//!
//! **What reaches the scan.** The pins are the raw's, so a fault acts on both buffers;
//! the memory map decides which addresses it reaches. On the A520 at native still size:
//!
//! - `a11` stuck low folds the scan down onto the raw's first words after the raw's
//!   read, and leaves the raw alone; written and read through the same fold, the scan
//!   comes back whole. A marginal `a11` low writes some bursts there and reads some
//!   from there: a burst written aside and read in place finds what the last frame that
//!   wrote it in place left, strips of an earlier picture, or the power-on content; one
//!   written in place and read aside finds this frame's raw, read as the codes its DMA
//!   wrote.
//! - `a11` stuck high sends the raw into the scan's words, where the scan overwrites it
//!   after the raw's read-back and the next frame's raw overwrites the scan in turn:
//!   neither sees the other. Marginal, it lets a raw line read aside find the last
//!   frame's scan, read as samples: each word's low `dsp_bits` bits above the nominal
//!   black.
//! - A pin whose bits lie below the scan's length folds the scan onto itself in powers
//!   of two, so the reader finds one chunk where two were written, and sends chunks
//!   into words the frame did not write.
//! - The movie raw, a quarter of the rows, ends at 967,872 words, below `a9`'s row bit
//!   2^20, and the movie's scan lies at `a11`'s, 2^22: `a9` to `a11` reach only the scan.
//!
//! The decoder reads the damaged scan as libjpeg does: a flipped bit mis-decodes a
//! Huffman code and the rest of the scan out of step, shifting, tinting and greying
//! the picture after it ([Decoding a damaged
//! scan](crate::codec#decoding-a-damaged-scan)).
//!
//! The write runs word by word in stream order, the read a burst per task. On the
//! development VM of `docs/ARCHITECTURE.md`'s timings, the bench's native superfine
//! scan of 1.7 MB takes about 1.6 ms through no fault, a stuck data pin or a board at
//! 120 °C, under half a percent of a frame; 3.5 ms through the S9000 tear's two
//! contacts (on its own 3.0 MB scan); and 2.9 to 3.6 ms laid on the raw's words by
//! `a11` stuck low, each word boxed into the raw's store.
//!
//! # Memory content
//!
//! [`State`] holds the SDRAM's words over the span the frame can reach: the raw buffer
//! `[lo, hi)` and wherever the faults can send its addresses. A fault changes only
//! the faulted bits, all below `H = 2^(h + 1)` for the highest faulted bit `h`, so the
//! span `[lo & !(H - 1), (hi - 1) | (H - 1)]` covers every address; the scan's span
//! follows the same way from its words ([`Params::scan_span`]). The stores cover every
//! span needed since power-on and keep their content across frames and across
//! [`Simulator::set_preset`](crate::Simulator::set_preset): new faults read the old
//! content, and a longer scan widens the scan's span as a new fault does. A word never
//! written holds the power-on content, uniform random 16-bit words hashed from the
//! address and the camera's `unit_seed`, of which a raw sample is the low `dsp_bits`
//! bits less the nominal black level and the scan's word all 16: a cell's power-up
//! state belongs to the chip, not the preset (PROVISIONAL: the A520 SDRAM's power-up
//! content is unknown).
//!
//! Two stores hold the words, so that each address has one word. The raw's store holds
//! one `f32` per word over the raw's span: a sample the raw wrote, or 16 bits boxed in
//! a NaN, which no sample is: the power-on content, or a word of the scan where the
//! scan's addresses fall in the raw's span. The raw reads a boxed word as its low
//! `dsp_bits` bits less the nominal black; the scan reads a sample as the code the DMA
//! wrote, `round(v + b)` with the nominal black, its high bits clear (the DMA drives
//! those pins low; a stuck high pin among them at the raw's write is not recorded in
//! the word, though the scan's own read through it sets the bit). The scan's store
//! holds the scan's words outside the raw's span, one `u16` each, with its own masks;
//! where the raw's span later grows over them, the raw's store takes them in, boxed.
//! Holding the scan in the raw's store alone would cost four bytes per word of the
//! scan, and the stretch between the two buffers besides.
//!
//! The raw's store costs four bytes per word, the scan's two, and the cells' masks of a
//! hot board four more each. At native resolution the still raw alone is 2272 by 1704
//! words, 15.5 MB; a fault on the top row pin `a11` widens its span to the whole
//! 2^23-word SDRAM, 33.6 MB, which then holds the scan too. The bench's 1.7 MB scan
//! adds a store of 1.7 MB; a fault on a lower pin widens the scan's span to the
//! 2^22 words above `jpeg_base`, 8.4 MB, beside the raw's 16.8 MB: half as much again
//! at most, and as much again for the masks of a hot board. A 640-wide preview needs
//! a sixteenth of that.
//!
//! # Resolution
//!
//! At a virtual geometry of `cols` by `rows` raw sites against the native `cols_n` by
//! `rows_n` (the native rows read in the mode), every linear weight is scaled and
//! rounded to a power of two: a column bit `c` becomes `c + round(log2(cols / cols_n))`
//! and a row bit `r` becomes `r + round(log2(cols * rows / (cols_n * rows_n)))`, and
//! `raw_base` and `jpeg_base` move with the row bits. A fold then spans the same
//! fraction of the width or height as natively, exactly when the ratios are powers of
//! two (568 by 426 against 2272 by 1704). A column bit other than bit 0 stays at least
//! bit 1, so a fold keeps the Bayer column phase it has natively; at small sizes
//! several native bits land on one, and the faults then apply in the preset's order.
//! The pitch is not a power of two, so a fold's row offset, and with it the Bayer row
//! phase of the folded copy, can differ between a virtual geometry and native
//! resolution.
//!
//! Where the row shift rounds down, the raw grows past the moved `jpeg_base`: at 160
//! by 120 its 19,200 words pass 2^14, at 640 by 480 its 307,200 pass 2^18. The scan's
//! first words then lie on the raw's last, overwriting them after the raw's read, and
//! the next frame's raw overwrites them after the scan's: neither sees the other, but
//! where a fault or a brownout leaves a word stale it holds the other buffer's.
//!
//! The data faults act on the bits of a sample and the cells on the bits of a word,
//! so neither needs scaling: a frame at any size takes the same stuck bits and loses
//! the same fraction of its sites to the heat. The true and anti cells follow the
//! scaled row bit 0, so their rows span the same fraction of the frame as natively.
//!
//! Not modelled: the SDRAM's burst counter (every word's column address passes the
//! pins), the auto-precharge role of `a10` in column commands, the packed layout of
//! the samples in the words, the interleaving of the next frame's writes with this
//! frame's reads and the scan's, and the wrap of the DMA's address counter at the
//! SDRAM's last word (a scan longer than the room above `jpeg_base` runs on into words
//! the stage adds).

use std::f64::consts::SQRT_2;
use std::fmt::{self, Debug, Formatter};
use std::ops::Range;

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::isp::Raw;
use crate::noise::{self, Pcg32};
use crate::preset::{AddressFault, DataFault, FaultKind, Mode};
use crate::readout::Direction;
use crate::{Camera, Error, Preset, Timing};

/// Salt of the stage seed, from which the contacts' streams derive.
const SALT: u32 = noise::salt("memory");

/// Salt of each marginal address contact's stream, combined with its pin.
const SALT_CONTACT: u32 = noise::salt("memory.contact");

/// Salt of each marginal data contact's stream, combined with its pin.
const SALT_DATA_CONTACT: u32 = noise::salt("memory.data-contact");

/// Salt of a contact's stream at the scan's instants that no raw line shares.
const SALT_SCAN_CONTACT: u32 = noise::salt("memory.scan-contact");

/// Salt of the SDRAM's power-on content, hashed with the camera's `unit_seed`.
const SALT_POWER_ON: u32 = noise::salt("memory.power-on");

/// Salt of the chip's map of weak cells, hashed with the camera's `unit_seed`.
const SALT_CELLS: u32 = noise::salt("memory.cells");

/// Scale that maps 32 random bits onto `[0, 1)`.
const UNIT_32: f64 = 1.0 / 4_294_967_296.0;

/// Values a 32-bit hash takes, `2^32`: the resolution of a cell's hashed uniform.
const HASH_VALUES: f64 = 4_294_967_296.0;

/// Most faults the per-line masks hold; each fault needs its own pin, and a camera has at most 16.
const MAX_FAULTS: usize = 32;

/// Most bridges among the data faults: each joins two of at most 16 data pins.
const MAX_BRIDGES: usize = 8;

/// Words of the store and of the cells' masks filled per parallel task.
const CHUNK_WORDS: usize = 4096;

/// Bits of a word of the scan: two bytes on the x16 part's data pins, the low byte first.
const WORD_BITS: u32 = 16;

/// Bit pattern of a raw store's word that holds 16 bits instead of a sample.
///
/// A quiet NaN, which no sample is, with payload bit 16 set, which the processor's
/// own NaNs (payload 0) lack; the word's 16 bits are the low 16 of the payload.
const BOXED: u32 = 0x7fc1_0000;

/// Bits of a raw store's word that [`BOXED`] fixes.
const BOX_MASK: u32 = 0xffff_0000;

/// Coefficients of the Chebyshev fit of `erfc`, constant term first.
///
/// Press, Teukolsky, Vetterling and Flannery, *Numerical Recipes in C* (2nd ed., 1992),
/// section 6.2, `erfcc`: `erfc(z) = t e^(-z² + P(t))` with `t = 1 / (1 + z / 2)`, a
/// fractional error below 1.2e-7 for every `z ≥ 0`, so the far tail keeps its
/// relative accuracy where the cells' decay is decided.
const ERFC_FIT: [f64; 10] = [
    -1.265_512_23,
    1.000_023_68,
    0.374_091_96,
    0.096_784_18,
    -0.186_288_06,
    0.278_868_07,
    -1.135_203_98,
    1.488_515_87,
    -0.822_152_23,
    0.170_872_77,
];

/// A marginal contact: a random telegraph process in camera time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// Fraction of the time the contact is faulted, in `(0, 1)`.
    pub on_frac: f64,
    /// Mean on-off cycles per second, at least 0.
    pub switch_hz: f64,
}

impl Contact {
    /// Returns the marginal contact of a fault's settings; `None` for a solid fault.
    fn of(on_frac: Option<f64>, switch_hz: Option<f64>) -> Option<Self> {
        match (on_frac, switch_hz) {
            (Some(on_frac), Some(switch_hz)) if on_frac < 1.0 => Some(Self { on_frac, switch_hz }),
            _ => None,
        }
    }

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

/// The bus a faulted pin belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bus {
    /// The multiplexed address pins, `a0` upward.
    Address,
    /// The data pins, `d0` upward.
    Data,
}

/// Where a fault sits: its bus, pins and kind, which a marginal contact's process follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wiring {
    bus: Bus,
    pin: u8,
    kind: FaultKind,
    with: Option<u8>,
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
        Self {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            bits,
            with_bits,
            contact: Contact::of(fault.on_frac, fault.switch_hz),
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

    /// Returns where the fault sits.
    fn wiring(&self) -> Wiring {
        Wiring {
            bus: Bus::Address,
            pin: self.pin,
            kind: self.kind,
            with: self.with,
        }
    }
}

/// One faulted data pin with the sample bits it acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct DataPin {
    /// The faulted pin, `d0` upward.
    pub pin: u8,
    /// What the fault does.
    pub kind: FaultKind,
    /// The other pin of a bridge.
    pub with: Option<u8>,
    /// The word's bits the fault acts on: the pin's, and a bridge's other pin's.
    ///
    /// 0 where the fault is inert: a stuck pin at or above the sample's `dsp_bits`, or
    /// a bridge with such a pin, whose OR with a bit that is never set changes nothing.
    pub bits: u32,
    /// The marginal contact; `None` for a solid fault.
    pub contact: Option<Contact>,
}

impl DataPin {
    /// Resolves one preset data fault on a word whose low `sample_bits` hold the sample.
    fn new(fault: &DataFault, sample_bits: u32) -> Self {
        let bit = |pin: u8| {
            let pin = u32::from(pin);
            if pin < sample_bits { 1u32 << pin } else { 0 }
        };
        let bits = match (fault.kind, fault.with) {
            (FaultKind::Bridge, Some(with)) if bit(fault.pin) != 0 && bit(with) != 0 => {
                bit(fault.pin) | bit(with)
            }
            (FaultKind::Bridge, _) => 0,
            (FaultKind::Low | FaultKind::High, _) => bit(fault.pin),
        };
        Self {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            bits,
            contact: Contact::of(fault.on_frac, fault.switch_hz),
        }
    }

    /// Returns whether the fault changes no word: its pins carry no sample bit.
    #[must_use]
    pub fn is_inert(&self) -> bool {
        self.bits == 0
    }

    /// Returns the word that crosses the pins for `word` while the fault acts.
    #[must_use]
    pub fn apply(&self, word: u32) -> u32 {
        match self.kind {
            FaultKind::Low => word & !self.bits,
            FaultKind::Bridge if word & self.bits == 0 => word,
            FaultKind::High | FaultKind::Bridge => word | self.bits,
        }
    }

    /// Returns where the fault sits.
    fn wiring(&self) -> Wiring {
        Wiring {
            bus: Bus::Data,
            pin: self.pin,
            kind: self.kind,
            with: self.with,
        }
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
        [
            (pin < self.column_bits).then(|| scaled(pin, self.column_shift)),
            (pin < self.row_bits).then(|| self.row_bit(pin)),
        ]
    }

    /// Returns the linear bit of row bit `bit`, scaled to the geometry.
    fn row_bit(&self, bit: u32) -> u32 {
        scaled(self.row_base + bit, self.row_shift)
    }
}

/// Returns linear bit `native` moved by `shift` octaves.
///
/// Bit 0 stays bit 0 and every other bit stays at least bit 1, so a fold keeps the
/// Bayer column phase it has natively.
fn scaled(native: u32, shift: i32) -> u32 {
    let floor = native.min(1);
    (i64::from(native) + i64::from(shift)).clamp(i64::from(floor), 62) as u32
}

/// Returns `round(log2(ratio))`.
fn octaves(ratio: f64) -> i32 {
    ratio.log2().round() as i32
}

/// The SDRAM's power-on content: hashed words, fixed for the unit.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
    ///
    /// The low `dsp_bits` bits of [`bits`](Self::bits), as the processor reads a sample.
    #[must_use]
    pub fn word(&self, address: u64) -> f32 {
        let code = self.hash(address) % (self.max_code + 1);
        code as f32 - self.black_code
    }

    /// Returns the 16 bits of the word at `address` before anything was written there.
    #[must_use]
    pub fn bits(&self, address: u64) -> u16 {
        self.hash(address) as u16
    }

    /// Returns the hash of the cells of the word at `address` at power-up.
    fn hash(&self, address: u64) -> u32 {
        noise::hash_u32(
            address as u32,
            (address >> 32) as u32,
            SALT_POWER_ON,
            self.seed,
        )
    }

    /// Returns the sample the processor reads from a word of the raw store.
    ///
    /// A word holding 16 bits reads as their low `dsp_bits` bits less the nominal black,
    /// as the power-on content does; a sample reads as itself.
    fn sample(&self, value: f32) -> f32 {
        match unboxed(value) {
            Some(bits) => (u32::from(bits) & self.max_code) as f32 - self.black_code,
            None => value,
        }
    }

    /// Returns the 16 bits the scan's reader finds in a word of the raw store.
    ///
    /// A sample is the code the DMA wrote, `round(v + b)` with the nominal black, its
    /// high pins driven low; a word holding 16 bits gives them.
    fn bits_of(&self, value: f32) -> u32 {
        unboxed(value).map_or_else(|| code(value, self.black_code, self.max_code), u32::from)
    }
}

/// Returns a raw store's word holding the 16 bits `bits`.
fn boxed(bits: u16) -> f32 {
    f32::from_bits(BOXED | u32::from(bits))
}

/// Returns the 16 bits a raw store's word holds, `None` if it holds a sample.
fn unboxed(value: f32) -> Option<u16> {
    let bits = value.to_bits();
    (bits & BOX_MASK == BOXED).then_some(bits as u16)
}

/// The bits of one word whose cells lose their charge before the refresh comes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Decay {
    /// Bits held by true cells that decay: they read 0 whatever was written.
    pub fall: u16,
    /// Bits held by anti cells that decay: they read 1 whatever was written.
    pub rise: u16,
}

impl Decay {
    /// Returns `word` as its cells hold it at the read.
    #[must_use]
    pub fn apply(self, word: u32) -> u32 {
        (word & !u32::from(self.fall)) | u32::from(self.rise)
    }

    /// Returns whether no cell of the word decays.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.fall == 0 && self.rise == 0
    }

    /// Returns the decayed cells of both masks.
    fn union(self, other: Self) -> Self {
        Self {
            fall: self.fall | other.fall,
            rise: self.rise | other.rise,
        }
    }
}

/// The SDRAM's heat and the cells that lose their bit before the refresh.
///
/// See the [module documentation](self#retention) for the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Retention {
    /// Temperature of the SDRAM in degrees Celsius.
    pub temperature_c: f64,
    /// Median retention of a cell at that temperature, in seconds.
    pub median_s: f64,
    /// Period in which the controller refreshes every row, in seconds.
    pub refresh_s: f64,
    /// Fraction of the cells whose retention falls short of the refresh period.
    pub decay_frac: f64,
    /// Seed of the chip's map of weak cells, from the camera's `unit_seed`.
    pub seed: u64,
    /// Bits of a word that hold the sample, from bit 0: the processor's `dsp_bits`.
    pub sample_bits: u32,
    /// Bits of a word the chip holds, from bit 0: its data pins, all 16 of the scan's word.
    pub word_bits: u32,
    /// Linear address bit of the SDRAM's row bit 0 at the geometry: odd rows hold anti cells.
    pub row_bit: u32,
    /// Hashes below which a cell decays, `ceil(decay_frac 2^32 - 1/2)` within `[0, 2^32]`.
    threshold: u64,
}

impl Retention {
    /// Returns whether no cell of the chip decays at this temperature.
    #[must_use]
    pub fn is_inert(&self) -> bool {
        self.threshold == 0
    }

    /// Returns whether the cell of bit `bit` of the word at `address` decays.
    ///
    /// The cell's hash `h` stands for the normal `N = Φ⁻¹((h + 1/2) / 2^32)`, and the
    /// cell decays when its retention falls short of the refresh period.
    #[must_use]
    pub fn decays(&self, address: u64, bit: u32) -> bool {
        let hash = noise::hash_u32(
            address as u32,
            (address >> 32) as u32,
            SALT_CELLS ^ bit,
            self.seed,
        );
        u64::from(hash) < self.threshold
    }

    /// Returns the sample bits of the word at `address` whose cells decay.
    ///
    /// The cells of the bits below `sample_bits`, those of a raw sample; they fall or
    /// rise by the parity of the word's SDRAM row.
    #[must_use]
    pub fn decay(&self, address: u64) -> Decay {
        self.decay_bits(address, 0..self.sample_bits)
    }

    /// Returns every bit of the word at `address` whose cell decays.
    ///
    /// The cells of the bits below `word_bits`, those of a word of the scan; they fall
    /// or rise by the parity of the word's SDRAM row, and those below `sample_bits` are
    /// [`decay`](Self::decay)'s.
    #[must_use]
    pub fn word_decay(&self, address: u64) -> Decay {
        self.decay_bits(address, 0..self.word_bits)
    }

    /// Returns the bits in `bits` of the word at `address` whose cells decay.
    fn decay_bits(&self, address: u64, bits: Range<u32>) -> Decay {
        if self.is_inert() {
            return Decay::default();
        }
        let bits = bits
            .filter(|&bit| self.decays(address, bit))
            .fold(0u16, |bits, bit| bits | 1 << bit);
        if address >> self.row_bit & 1 == 0 {
            Decay {
                fall: bits,
                rise: 0,
            }
        } else {
            Decay {
                fall: 0,
                rise: bits,
            }
        }
    }

    /// Returns what decides the decayed cells of the low `bits` bits, apart from the addresses.
    fn map(&self, bits: u32) -> CellMap {
        CellMap {
            seed: self.seed,
            threshold: self.threshold,
            bits,
            row_bit: self.row_bit,
        }
    }
}

/// Returns the standard normal distribution function `Φ(x)`, through [`ERFC_FIT`].
fn normal_cdf(x: f64) -> f64 {
    let z = x.abs() / SQRT_2;
    let t = 1.0 / (1.0 + 0.5 * z);
    let fit = ERFC_FIT
        .iter()
        .rev()
        .fold(0.0_f64, |sum, &c| sum.mul_add(t, c));
    // Half of erfc(|x| / sqrt 2): the tail beyond |x|.
    let tail = 0.5 * t * (fit - z * z).exp();
    if x < 0.0 { tail } else { 1.0 - tail }
}

/// The resolved faults, heat and buffer layout, present with `[bend.memory]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Bits of the SDRAM's linear word address: column, row and bank bits together.
    pub address_bits: u32,
    /// Bendable address pins, `a0` to `a(pins - 1)`: `max(column_bits, row_bits)`.
    pub pins: u32,
    /// Data pins of the SDRAM, `d0` to `d(data_pins - 1)`.
    pub data_pins: u32,
    /// Bits of the processor's sample, held in the low bits of a word: its `dsp_bits`.
    pub sample_bits: u32,
    /// Word address of the raw buffer's first sample at this geometry.
    pub raw_base: u64,
    /// Word address of the compressed scan's first word at this geometry.
    pub jpeg_base: u64,
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
    /// The faulted address pins in the preset's order, with their bits at this geometry.
    pub faults: Vec<Fault>,
    /// The faulted data pins in the preset's order, with the sample bits they act on.
    pub data_faults: Vec<DataPin>,
    /// The same pins acting on the scan's words, which use all 16 bits: none is inert.
    pub scan_data_faults: Vec<DataPin>,
    /// The SDRAM's temperature and the cells that lose their bit at it.
    pub retention: Retention,
    /// The word addresses the raw can reach: its buffer and every faulted address.
    pub span: Range<u64>,
    /// Seed of the contacts' streams.
    pub seed: u64,
    /// What a word holds before it is first written.
    pub power_on: PowerOn,
    /// Whether the compressed scan lies within the faults' reach: `[bend.memory] reaches_jpeg`.
    ///
    /// The simulator puts the scan through [`bend_scan`] only while it is set.
    pub reaches_jpeg: bool,
}

impl Params {
    /// Resolves the section against the camera's `[memory]`; `None` without `[bend.memory]`.
    ///
    /// The bits each address pin drives are scaled from the native readout to the
    /// geometry of `timing` as the [module documentation](self) describes, and
    /// `raw_base` and `jpeg_base` move with the row bits; the data pins and the cells
    /// need no scaling. Without `temperature_c` the SDRAM runs at the camera's
    /// `reference_temp_c`.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a fault is invalid on its own, names an address
    /// pin beyond `a(max(column_bits, row_bits) - 1)` (the bank pins are not bendable)
    /// or a data pin beyond `d(data_pins - 1)`, or if the native raw buffer does not
    /// fit in the SDRAM above `raw_base`.
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
        let dsp_bits = u32::from(camera.adc.dsp_bits.min(16));
        let data_faults = data_faults(&bend.data_faults, camera, dsp_bits)?;

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
        let shifted = |base: u64| {
            if row_shift >= 0 {
                base << row_shift.unsigned_abs()
            } else {
                base >> row_shift.unsigned_abs()
            }
        };
        let raw_base = shifted(memory.raw_base);
        let faults: Vec<Fault> = bend
            .faults
            .iter()
            .map(|fault| Fault::new(fault, &layout))
            .collect();

        let end = raw_base + u64::from(cols) * u64::from(rows);
        let below = reach(&faults);
        let span = (raw_base & !below)..((end - 1) | below) + 1;

        let temperature_c = bend.temperature_c.unwrap_or(memory.reference_temp_c);
        let retention = retention(camera, temperature_c, dsp_bits, layout.row_bit(0));

        Ok(Some(Self {
            address_bits,
            pins,
            data_pins: u32::from(memory.data_pins),
            sample_bits: dsp_bits,
            raw_base,
            jpeg_base: shifted(memory.jpeg_base),
            pitch: cols,
            rows,
            direction: camera.clocking(timing.mode).direction,
            line_period_s: timing.line_period_s,
            frame_period_s: timing.frame_period_s,
            faults,
            scan_data_faults: bend
                .data_faults
                .iter()
                .map(|fault| DataPin::new(fault, WORD_BITS))
                .collect(),
            data_faults,
            retention,
            span,
            seed: noise::stage_seed(preset.seed, SALT),
            power_on: PowerOn {
                seed: noise::stage_seed(camera.sensor.unit_seed, SALT_POWER_ON),
                max_code: (1u32 << dsp_bits) - 1,
                black_code: f32::from(camera.adc.black_level >> camera.adc.dsp_lsb),
            },
            reaches_jpeg: bend.reaches_jpeg,
        }))
    }

    /// Returns the linear word address of raw site `(x, y)`, rows in image order.
    #[must_use]
    pub fn address(&self, x: u32, y: u32) -> u64 {
        self.raw_base + u64::from(y) * u64::from(self.pitch) + u64::from(x)
    }

    /// Returns the word addresses a scan of `bytes` bytes can reach.
    ///
    /// Its words lie from `jpeg_base` on, two bytes a word, and the faults send them
    /// anywhere their bits reach: they change only the bits below and at the highest
    /// faulted one, as for the raw's [`span`](Self::span). Empty for an empty scan.
    #[must_use]
    pub fn scan_span(&self, bytes: usize) -> Range<u64> {
        let words = (bytes as u64).div_ceil(2);
        if words == 0 {
            return self.jpeg_base..self.jpeg_base;
        }
        let below = reach(&self.faults);
        let end = self.jpeg_base + words;
        (self.jpeg_base & !below)..((end - 1) | below) + 1
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

    /// Returns `word` as it crosses the data pins while the data faults in `active` act.
    ///
    /// Bit `i` of `active` stands for `data_faults[i]`.
    #[must_use]
    pub fn cross(&self, word: u32, active: u32) -> u32 {
        Pins::new(&self.data_faults, active).apply(word)
    }

    /// Returns every marginal contact, address and data, with where it sits.
    fn marginal(&self) -> impl Iterator<Item = (Wiring, Contact)> + '_ {
        let address = self
            .faults
            .iter()
            .filter_map(|fault| fault.contact.map(|contact| (fault.wiring(), contact)));
        let data = self
            .data_faults
            .iter()
            .filter_map(|fault| fault.contact.map(|contact| (fault.wiring(), contact)));
        address.chain(data)
    }

    /// Returns the seed of the stream of the contact at `wiring`.
    fn contact_seed(&self, wiring: Wiring) -> u64 {
        let salt = match wiring.bus {
            Bus::Address => SALT_CONTACT,
            Bus::Data => SALT_DATA_CONTACT,
        };
        noise::stage_seed(self.seed, salt ^ u32::from(wiring.pin))
    }

    /// Returns whether a data fault changes any word.
    fn bends_data(&self) -> bool {
        self.data_faults.iter().any(|fault| !fault.is_inert())
    }

    /// Returns the time from the last line's read of a frame to the first line's of the next.
    fn gap_s(&self) -> f64 {
        self.frame_period_s - self.line_period_s * f64::from(self.rows - 1)
    }
}

/// Returns the linear address bits below and at the highest bit a fault can change.
fn reach(faults: &[Fault]) -> u64 {
    match faults.iter().fold(0, |mask, fault| mask | fault.mask()) {
        0 => 0,
        mask => u64::MAX >> mask.leading_zeros(),
    }
}

/// Resolves the data faults on a word whose low `sample_bits` hold the sample.
///
/// # Errors
///
/// Returns a configuration error naming the first fault whose pin or bridge partner
/// lies beyond the camera's data pins.
fn data_faults(
    faults: &[DataFault],
    camera: &Camera,
    sample_bits: u32,
) -> Result<Vec<DataPin>, Error> {
    let data_pins = u32::from(camera.memory.data_pins);
    for (index, fault) in faults.iter().enumerate() {
        for (key, pin) in [("pin", Some(fault.pin)), ("with", fault.with)] {
            if let Some(pin) = pin.filter(|&pin| u32::from(pin) >= data_pins) {
                return Err(Error::config(format!(
                    "bend.memory.data_faults[{index}].{key} names pin d{pin}, but the camera's \
                     SDRAM has the data pins d0 to d{}",
                    data_pins - 1
                )));
            }
        }
    }
    debug_assert!(
        faults.len() <= MAX_FAULTS,
        "{} data faults on {data_pins} pins passed validation",
        faults.len()
    );
    Ok(faults
        .iter()
        .map(|fault| DataPin::new(fault, sample_bits))
        .collect())
}

/// Resolves the cells' retention at `temperature_c` on the camera's SDRAM.
fn retention(camera: &Camera, temperature_c: f64, sample_bits: u32, row_bit: u32) -> Retention {
    let memory = &camera.memory;
    let refresh_s = memory.refresh_ms / 1000.0;
    let median_s = memory.retention_s
        * (-(temperature_c - memory.reference_temp_c) / memory.retention_halving_c).exp2();
    let decay_frac = if memory.retention_spread > 0.0 {
        normal_cdf((refresh_s / median_s).ln() / memory.retention_spread)
    } else if median_s < refresh_s {
        1.0
    } else {
        0.0
    };
    // (h + 1/2) / 2^32 < p for the hashes h below ceil(p 2^32 - 1/2).
    let threshold = (decay_frac * HASH_VALUES - 0.5)
        .ceil()
        .clamp(0.0, HASH_VALUES) as u64;
    Retention {
        temperature_c,
        median_s,
        refresh_s,
        decay_frac,
        seed: noise::stage_seed(camera.sensor.unit_seed, SALT_CELLS),
        sample_bits,
        word_bits: u32::from(memory.data_pins).min(WORD_BITS),
        row_bit,
        threshold,
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

/// The data faults acting on one burst, composed: on disjoint pins they commute.
#[derive(Debug, Clone, Copy)]
struct Pins {
    /// Bits no stuck-low pin clears.
    keep: u32,
    /// Bits a stuck-high pin sets.
    set: u32,
    /// The bridges' pairs of bits.
    bridges: [u32; MAX_BRIDGES],
    len: usize,
}

impl Pins {
    /// Composes the data faults whose bit is set in `active`, leaving out the inert ones.
    fn new(faults: &[DataPin], active: u32) -> Self {
        let mut pins = Self {
            keep: u32::MAX,
            set: 0,
            bridges: [0; MAX_BRIDGES],
            len: 0,
        };
        for (i, fault) in faults.iter().enumerate() {
            if active >> i & 1 == 0 || fault.is_inert() {
                continue;
            }
            match fault.kind {
                FaultKind::Low => pins.keep &= !fault.bits,
                FaultKind::High => pins.set |= fault.bits,
                FaultKind::Bridge => {
                    pins.bridges[pins.len] = fault.bits;
                    pins.len += 1;
                }
            }
        }
        pins
    }

    /// Returns whether no fault changes a word.
    fn is_identity(&self) -> bool {
        self.keep == u32::MAX && self.set == 0 && self.len == 0
    }

    /// Returns the word that crosses the pins for `word`.
    fn apply(&self, word: u32) -> u32 {
        let word = (word & self.keep) | self.set;
        self.bridges[..self.len]
            .iter()
            .fold(word, |w, &bits| if w & bits == 0 { w } else { w | bits })
    }
}

/// Returns the word the DMA writes for `value` of a row whose black level is `black`.
///
/// `round(v + b)` held within `0..=max_code`. The sum widened to `f64` takes the half
/// exactly, and the conversion truncates and saturates below 0, so this rounds as
/// `f32::round` does on the codes without its library call.
fn code(value: f32, black: f32, max_code: u32) -> u32 {
    ((f64::from(value + black) + 0.5) as u32).min(max_code)
}

/// Returns the value stored for `value` of a row with black `black`, written through `pins`.
///
/// The faulted code less the row's black; the value itself where no bit changes.
fn write_word(value: f32, black: f32, pins: &Pins, max_code: u32) -> f32 {
    let word = code(value, black, max_code);
    let written = pins.apply(word);
    if written == word {
        value
    } else {
        written as f32 - black
    }
}

/// Returns the stored `value` as a row with black `black` reads it, through its cells and `pins`.
///
/// The value changes by the weights of the bits that change.
fn read_word(value: f32, black: f32, decay: Decay, pins: &Pins, max_code: u32) -> f32 {
    let word = code(value, black, max_code);
    let read = pins.apply(decay.apply(word));
    if read == word {
        value
    } else {
        value + (read as f32 - word as f32)
    }
}

/// The raw's store: the SDRAM's words over the addresses the raw has reached.
///
/// A word holds a sample the raw wrote, or [boxed](boxed) 16 bits: the power-on
/// content, or a word of the scan.
#[derive(Clone, Default)]
struct Sdram {
    /// Address of `words[0]`.
    start: u64,
    words: Vec<f32>,
}

impl PartialEq for Sdram {
    /// Compares the words bit for bit: a boxed word is a NaN.
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
            && self.words.len() == other.words.len()
            && self
                .words
                .iter()
                .zip(&other.words)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

impl Sdram {
    /// Returns the addresses the store covers.
    fn span(&self) -> Range<u64> {
        self.start..self.start + self.words.len() as u64
    }

    /// Widens the store to cover `span`.
    ///
    /// A new word takes the scan's store's word where that holds it, the power-on content
    /// elsewhere.
    fn cover(&mut self, span: &Range<u64>, power_on: &PowerOn, scan: &Words) {
        let held = scan.span();
        widen(&mut self.start, &mut self.words, span, |address| {
            boxed(if held.contains(&address) {
                scan.words[scan.slot(address)]
            } else {
                power_on.bits(address)
            })
        });
    }

    /// Returns the index of `address` in `words`.
    fn slot(&self, address: u64) -> usize {
        (address - self.start) as usize
    }
}

/// The scan's store: its words outside the raw's store, 16 bits each.
///
/// A word inside the raw's store's span is the raw store's; its copy here, if any, is
/// never read.
#[derive(Clone, Default, PartialEq)]
struct Words {
    /// Address of `words[0]`.
    start: u64,
    words: Vec<u16>,
}

impl Words {
    /// Returns the addresses the store covers.
    fn span(&self) -> Range<u64> {
        self.start..self.start + self.words.len() as u64
    }

    /// Widens the store to cover `span`, filling new words with the power-on content.
    fn cover(&mut self, span: &Range<u64>, power_on: &PowerOn) {
        widen(&mut self.start, &mut self.words, span, |address| {
            power_on.bits(address)
        });
    }

    /// Returns the index of `address` in `words`.
    fn slot(&self, address: u64) -> usize {
        (address - self.start) as usize
    }
}

/// Widens a store, `words` from address `start` on, to cover `span`, filling new words with `fill`.
///
/// The store keeps its words and grows to the smallest span covering both.
fn widen<T: Copy + Default + Send + Sync>(
    start: &mut u64,
    words: &mut Vec<T>,
    span: &Range<u64>,
    fill: impl Fn(u64) -> T + Sync,
) {
    let have = *start..*start + words.len() as u64;
    if span.is_empty() || (!words.is_empty() && have.start <= span.start && span.end <= have.end) {
        return;
    }
    let new = if words.is_empty() {
        span.clone()
    } else {
        have.start.min(span.start)..have.end.max(span.end)
    };
    let len = usize::try_from(new.end - new.start).expect("the SDRAM span fits in memory");
    let mut widened = vec![T::default(); len];
    let from = new.start;
    widened
        .par_chunks_mut(CHUNK_WORDS)
        .enumerate()
        .for_each(|(chunk, slots)| {
            let first = from + (chunk * CHUNK_WORDS) as u64;
            for (offset, slot) in (0u64..).zip(slots.iter_mut()) {
                *slot = fill(first + offset);
            }
        });
    if !words.is_empty() {
        let at = (have.start - from) as usize;
        widened[at..at + words.len()].copy_from_slice(words);
    }
    *start = from;
    *words = widened;
}

/// Returns the part of `span` outside `held`, as one span; `None` if `held` covers it.
///
/// Where `held` lies strictly inside `span`, the whole of `span`.
fn outside(span: &Range<u64>, held: &Range<u64>) -> Option<Range<u64>> {
    if span.is_empty() {
        return None;
    }
    if held.is_empty() {
        return Some(span.clone());
    }
    match (span.start < held.start, span.end > held.end) {
        (false, false) => None,
        (true, false) => Some(span.start..span.end.min(held.start)),
        (false, true) => Some(span.start.max(held.end)..span.end),
        (true, true) => Some(span.clone()),
    }
}

/// What decides which cells of the chip decay, apart from their addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellMap {
    seed: u64,
    threshold: u64,
    /// Bits of each word whose cells the masks hold, from bit 0.
    bits: u32,
    row_bit: u32,
}

/// The decayed cells of a store's words at the SDRAM's temperature.
#[derive(Clone, Default, PartialEq)]
struct Cells {
    /// The map and the store's span the masks were computed for; `None` before the first frame.
    key: Option<(CellMap, Range<u64>)>,
    /// Each word's decayed bits, `masks[i]` for the store's `words[i]`; empty while no cell decays.
    masks: Vec<Decay>,
}

impl Cells {
    /// Computes the masks of the low `bits` bits over `span` unless they hold already.
    ///
    /// Masks of the same cells over a span inside `span` are kept, and only the words
    /// the span adds are computed.
    fn update(&mut self, retention: &Retention, span: Range<u64>, bits: u32) {
        let key = (retention.map(bits), span);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        if retention.is_inert() {
            self.masks = Vec::new();
        } else {
            let (map, span) = (&key.0, &key.1);
            let len =
                usize::try_from(span.end - span.start).expect("the SDRAM span fits in memory");
            let kept = self
                .key
                .as_ref()
                .filter(|(old_map, old)| {
                    old_map == map
                        && !self.masks.is_empty()
                        && span.start <= old.start
                        && old.end <= span.end
                })
                .map(|(_, old)| old.clone());
            if let Some(old) = kept {
                let mut masks = vec![Decay::default(); len];
                let at = (old.start - span.start) as usize;
                let end = at + self.masks.len();
                masks[at..end].copy_from_slice(&self.masks);
                fill_masks(&mut masks[..at], span.start, retention, bits);
                fill_masks(&mut masks[end..], old.end, retention, bits);
                self.masks = masks;
            } else {
                self.masks.clear();
                self.masks.resize(len, Decay::default());
                fill_masks(&mut self.masks, span.start, retention, bits);
            }
        }
        self.key = Some(key);
    }

    /// Returns the masks, `None` while no cell decays.
    fn masks(&self) -> Option<&[Decay]> {
        (!self.masks.is_empty()).then_some(self.masks.as_slice())
    }
}

/// Fills `masks` with the decayed cells of the low `bits` bits of the words from `start` on.
fn fill_masks(masks: &mut [Decay], start: u64, retention: &Retention, bits: u32) {
    masks
        .par_chunks_mut(CHUNK_WORDS)
        .enumerate()
        .for_each(|(chunk, slots)| {
            let first = start + (chunk * CHUNK_WORDS) as u64;
            for (offset, slot) in (0u64..).zip(slots.iter_mut()) {
                *slot = retention.decay_bits(first + offset, 0..bits);
            }
        });
}

/// One marginal contact's process, sampled at every readout line or every burst of the scan.
#[derive(Debug, Clone, PartialEq)]
struct ContactState {
    wiring: Wiring,
    /// Whether the contact was faulted at each readout line's (or burst's) write of the last frame.
    write: Vec<bool>,
    /// Whether it was faulted at each line's (or burst's) read; a line's is the next frame's write.
    read: Vec<bool>,
}

/// The SDRAM's memory from frame to frame; the default is the power-on state.
///
/// Holds the content of every word the frames have reached, the raw's and the scan's,
/// the masks of the cells that decay at the SDRAM's temperature and the marginal
/// contacts' states, address and data. A store is allocated with the first frame and
/// widened when a new fault or a longer scan reaches further, the masks with the first
/// frame at a temperature where cells decay, never in the steady state.
#[derive(Clone, Default, PartialEq)]
pub struct State {
    sdram: Sdram,
    cells: Cells,
    contacts: Vec<ContactState>,
    /// The address faults acting at each readout line's write, bit `i` for fault `i`.
    write_active: Vec<u32>,
    /// The address faults acting at each readout line's read.
    read_active: Vec<u32>,
    /// The data faults acting at each readout line's write, bit `i` for data fault `i`.
    data_write_active: Vec<u32>,
    /// The data faults acting at each readout line's read.
    data_read_active: Vec<u32>,
    /// The frame's black level of each image row, which the words' codes add back.
    black: Vec<f32>,
    /// The scan's words outside the raw's store.
    scan: Words,
    /// The decayed cells of the scan's store, every bit of a word.
    scan_cells: Cells,
    /// Every marginal contact's samples at the last scan's bursts.
    scan_contacts: Vec<ContactState>,
    /// The address faults acting at each burst's write of the last scan.
    scan_write_active: Vec<u32>,
    /// The address faults acting at each burst's read of the last scan.
    scan_read_active: Vec<u32>,
    /// The data faults acting at each burst's write of the last scan.
    scan_data_write_active: Vec<u32>,
    /// The data faults acting at each burst's read of the last scan.
    scan_data_read_active: Vec<u32>,
    /// The power-on content and sample layout of the last frame's camera.
    power_on: PowerOn,
}

impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("span", &self.sdram.span())
            .field("scan_span", &self.scan.span())
            .field("decay_masks", &self.cells.masks.len())
            .field("scan_decay_masks", &self.scan_cells.masks.len())
            .field("contacts", &self.contacts.len())
            .finish_non_exhaustive()
    }
}

impl State {
    /// Returns the sample the raw reads at `address`, cells and pins aside, for tests.
    ///
    /// `None` outside the raw's store. A word the raw did not write reads as its low
    /// `dsp_bits` bits less the nominal black: the power-on content, or the scan's word.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn word(&self, address: u64) -> Option<f32> {
        self.sdram.span().contains(&address).then(|| {
            self.power_on
                .sample(self.sdram.words[self.sdram.slot(address)])
        })
    }

    /// Returns the 16 bits the scan reads at `address`, cells and pins aside, for tests.
    ///
    /// `None` outside both stores. A raw sample reads as the code its DMA wrote with
    /// the nominal black, its high bits clear.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn scan_word(&self, address: u64) -> Option<u16> {
        if self.sdram.span().contains(&address) {
            let value = self.sdram.words[self.sdram.slot(address)];
            Some(self.power_on.bits_of(value) as u16)
        } else {
            self.scan
                .span()
                .contains(&address)
                .then(|| self.scan.words[self.scan.slot(address)])
        }
    }

    /// Returns the addresses the raw's store covers, empty before the first frame, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn span(&self) -> Range<u64> {
        self.sdram.span()
    }

    /// Returns the addresses the scan's store covers, empty before the first scan, for tests.
    ///
    /// The scan's words inside [`span`](Self::span) are the raw store's.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn scan_span(&self) -> Range<u64> {
        self.scan.span()
    }

    /// Returns the decayed cells the stage holds for the word at `address`, for tests.
    ///
    /// `None` outside the store; the masks are those of the temperature of the last
    /// frame, empty where no cell decays.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn decay(&self, address: u64) -> Option<Decay> {
        self.sdram.span().contains(&address).then(|| {
            self.cells
                .masks()
                .map_or(Decay::default(), |masks| masks[self.sdram.slot(address)])
        })
    }

    /// Returns the samples of the address contact on `pin` in the last frame, for tests.
    ///
    /// Whether the contact was faulted at each line's write and at its read, indexed by
    /// readout line; `None` if no marginal contact sits on address pin `pin`.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn contact_lines(&self, pin: u8) -> Option<(&[bool], &[bool])> {
        self.lines(Bus::Address, pin)
    }

    /// Returns the samples of the data contact on `pin` in the last frame, for tests.
    ///
    /// Whether the contact was faulted at each line's write and at its read, indexed by
    /// readout line; `None` if no marginal contact sits on data pin `pin`.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn data_contact_lines(&self, pin: u8) -> Option<(&[bool], &[bool])> {
        self.lines(Bus::Data, pin)
    }

    /// Returns the samples of the address contact on `pin` at the last scan, for tests.
    ///
    /// Whether the contact was faulted at each burst's write and at its read, indexed
    /// by burst; `None` if no marginal contact sits on address pin `pin`.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn scan_contact_bursts(&self, pin: u8) -> Option<(&[bool], &[bool])> {
        samples(&self.scan_contacts, Bus::Address, pin)
    }

    /// Returns the samples of the data contact on `pin` at the last scan, for tests.
    ///
    /// Whether the contact was faulted at each burst's write and at its read, indexed
    /// by burst; `None` if no marginal contact sits on data pin `pin`.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn scan_data_contact_bursts(&self, pin: u8) -> Option<(&[bool], &[bool])> {
        samples(&self.scan_contacts, Bus::Data, pin)
    }

    /// Returns the samples of the contact on `pin` of `bus`.
    #[cfg(feature = "test-util")]
    fn lines(&self, bus: Bus, pin: u8) -> Option<(&[bool], &[bool])> {
        samples(&self.contacts, bus, pin)
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
                    .marginal()
                    .any(|(wiring, _)| wiring == contact.wiring)
        });
        let gap_s = params.gap_s();
        for (wiring, contact) in params.marginal() {
            let mut rng = Pcg32::new(params.contact_seed(wiring), index);
            let at = if let Some(at) = self
                .contacts
                .iter()
                .position(|state| state.wiring == wiring)
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
                    wiring,
                    write,
                    read: vec![false; rows],
                });
                self.contacts.len() - 1
            };
            let state = &mut self.contacts[at];
            draw_reads(
                contact,
                &mut rng,
                state.write[rows - 1],
                gap_s,
                params.line_period_s,
                &mut state.read,
            );
        }

        mark(
            &self.contacts,
            params
                .faults
                .iter()
                .map(|fault| (fault.wiring(), fault.contact.is_some())),
            rows,
            &mut self.write_active,
            &mut self.read_active,
        );
        mark(
            &self.contacts,
            params
                .data_faults
                .iter()
                .map(|fault| (fault.wiring(), fault.contact.is_some())),
            rows,
            &mut self.data_write_active,
            &mut self.data_read_active,
        );
    }
}

/// Draws a contact's samples at a frame's line reads, from its sample at the frame's last write.
///
/// The first read follows the last write by the gap `gap_s`, every later one the one
/// before by `line_s`.
fn draw_reads(
    contact: Contact,
    rng: &mut Pcg32,
    last_write: bool,
    gap_s: f64,
    line_s: f64,
    reads: &mut [bool],
) {
    let mut on = last_write;
    for (line, slot) in reads.iter_mut().enumerate() {
        let dt_s = if line == 0 { gap_s } else { line_s };
        on = chance(rng, contact.on_probability(on, dt_s));
        *slot = on;
    }
}

/// Returns the write and read samples of the contact on `pin` of `bus` among `contacts`.
#[cfg(feature = "test-util")]
fn samples(contacts: &[ContactState], bus: Bus, pin: u8) -> Option<(&[bool], &[bool])> {
    contacts
        .iter()
        .find(|contact| contact.wiring.bus == bus && contact.wiring.pin == pin)
        .map(|contact| (contact.write.as_slice(), contact.read.as_slice()))
}

impl State {
    /// Samples every marginal contact at the write and read of each of the scan's `bursts`.
    fn sample_scan_contacts(&mut self, params: &Params, index: u64, bursts: usize) {
        let rows = params.rows as usize;
        self.scan_contacts.retain(|contact| {
            params
                .marginal()
                .any(|(wiring, _)| wiring == contact.wiring)
        });
        for (wiring, contact) in params.marginal() {
            let at = if let Some(at) = self
                .scan_contacts
                .iter()
                .position(|state| state.wiring == wiring)
            {
                at
            } else {
                self.scan_contacts.push(ContactState {
                    wiring,
                    write: Vec::with_capacity(bursts),
                    read: Vec::with_capacity(bursts),
                });
                self.scan_contacts.len() - 1
            };
            // This frame's raw reads, if the raw ran through the contact as it is.
            let raw = self
                .contacts
                .iter()
                .find(|state| state.wiring == wiring && state.read.len() == rows)
                .map(|state| state.read.as_slice());
            let state = &mut self.scan_contacts[at];
            state.write.clear();
            state.write.resize(bursts, false);
            state.read.clear();
            state.read.resize(bursts, false);
            sample_bursts(
                params,
                wiring,
                contact,
                raw,
                index,
                &mut state.write,
                &mut state.read,
            );
        }

        mark(
            &self.scan_contacts,
            params
                .faults
                .iter()
                .map(|fault| (fault.wiring(), fault.contact.is_some())),
            bursts,
            &mut self.scan_write_active,
            &mut self.scan_read_active,
        );
        mark(
            &self.scan_contacts,
            params
                .data_faults
                .iter()
                .map(|fault| (fault.wiring(), fault.contact.is_some())),
            bursts,
            &mut self.scan_data_write_active,
            &mut self.scan_data_read_active,
        );
    }
}

/// Samples one marginal contact at the write and read of each burst of the scan.
///
/// `raw` holds the contact's samples at this frame's line reads, whose instants burst
/// `k`'s write shares; burst `k`'s read shares the next frame's line `k` read, drawn
/// here from the next frame's stream as that frame's [`bend`] draws it. The instants
/// no line shares, of the bursts past the last line, continue the path from the latest
/// sample before them on a stream of the scan's own; without `raw` every instant does,
/// from a power-up at the first write.
fn sample_bursts(
    params: &Params,
    wiring: Wiring,
    contact: Contact,
    raw: Option<&[bool]>,
    index: u64,
    write: &mut [bool],
    read: &mut [bool],
) {
    let bursts = write.len();
    let rows = params.rows as usize;
    let (line_s, frame_s) = (params.line_period_s, params.frame_period_s);
    // Each line's and burst's time from the frame's first write, to a frame period.
    let at = |period: f64, k: usize| period.mul_add(frame_s, k as f64 * line_s);
    let mut lines = Vec::new();
    let first_own = if let Some(raw) = raw {
        let shared = bursts.min(rows);
        write[..shared].copy_from_slice(&raw[..shared]);
        let mut next = vec![false; if bursts > rows { rows } else { shared }];
        let mut stream = Pcg32::new(params.contact_seed(wiring), index.wrapping_add(1));
        draw_reads(
            contact,
            &mut stream,
            raw[rows - 1],
            params.gap_s(),
            line_s,
            &mut next,
        );
        read[..shared].copy_from_slice(&next[..shared]);
        if bursts <= rows {
            return;
        }
        lines.extend(raw.iter().enumerate().map(|(k, &on)| (at(1.0, k), on)));
        lines.extend(next.iter().enumerate().map(|(k, &on)| (at(2.0, k), on)));
        rows
    } else {
        0
    };
    // The instants of the bursts from `first_own` on, writes and reads, in time order.
    let mut own: Vec<(f64, usize, bool)> = (first_own..bursts)
        .flat_map(|k| [(at(1.0, k), k, false), (at(2.0, k), k, true)])
        .collect();
    own.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut rng = Pcg32::new(
        noise::stage_seed(params.contact_seed(wiring), SALT_SCAN_CONTACT),
        index,
    );
    let mut lines = lines.into_iter().peekable();
    let mut last: Option<(f64, bool)> = None;
    for (t_s, burst, is_read) in own {
        while let Some(&line) = lines.peek().filter(|&&(line_t_s, _)| line_t_s <= t_s) {
            last = Some(line);
            lines.next();
        }
        let on = match last {
            None => chance(&mut rng, contact.on_frac),
            Some((last_t_s, was)) => chance(&mut rng, contact.on_probability(was, t_s - last_t_s)),
        };
        last = Some((t_s, on));
        if is_read {
            read[burst] = on;
        } else {
            write[burst] = on;
        }
    }
}

/// Sets bit `i` of each of the `rows` lines' write and read masks where fault `i` acts.
///
/// `faults` gives each fault's wiring and whether it is a marginal contact, whose
/// samples `contacts` holds; a solid fault acts at every line.
fn mark(
    contacts: &[ContactState],
    faults: impl Iterator<Item = (Wiring, bool)>,
    rows: usize,
    write: &mut Vec<u32>,
    read: &mut Vec<u32>,
) {
    write.clear();
    read.clear();
    write.resize(rows, 0);
    read.resize(rows, 0);
    for (i, (wiring, marginal)) in faults.enumerate() {
        let bit = 1u32 << i;
        let contact = marginal
            .then(|| contacts.iter().find(|state| state.wiring == wiring))
            .flatten();
        for line in 0..rows {
            let (on_write, on_read) =
                contact.map_or((true, true), |state| (state.write[line], state.read[line]));
            if on_write {
                write[line] |= bit;
            }
            if on_read {
                read[line] |= bit;
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
/// processor captured is stored in readout order, one row at a time, through the
/// address and data faults acting at its write, then the whole frame is read back a
/// frame period later through the cells at the SDRAM's temperature and the faults
/// acting then; see the [module documentation](self). The rows from
/// [`Raw::dropped_from`] on, in readout order, are not stored: their read finds the
/// SDRAM's earlier content. Without faults, decayed cells or a brownout the raw comes
/// back unchanged.
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
    state.power_on = params.power_on;
    state
        .sdram
        .cover(&params.span, &params.power_on, &state.scan);
    state
        .cells
        .update(&params.retention, state.sdram.span(), params.sample_bits);
    state.sample_contacts(params, index);
    let bends_data = params.bends_data();
    if bends_data || state.cells.masks().is_some() {
        state.black.clear();
        state.black.extend_from_slice(raw.black_levels());
    }

    // From the first line the processor dropped on, nothing reaches the SDRAM.
    let captured = raw.dropped_from().map_or(params.rows, |y| params.line(y));
    if bends_data {
        fault_writes(params, state, captured, raw);
    }
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
    let load = Load {
        params,
        sdram: &state.sdram,
        masks: state.cells.masks(),
        read_active: &state.read_active,
        data_write_active: &state.data_write_active,
        data_read_active: &state.data_read_active,
        black: &state.black,
        reread: in_place < params.rows,
    };
    raw.plane_mut()
        .data_mut()
        .par_chunks_mut(params.pitch as usize)
        .enumerate()
        .for_each(|(y, sites)| load_row(&load, y as u32, sites));
}

/// Rewrites frame `index`'s `scan` as the card writer reads it back from the SDRAM.
///
/// `scan` holds the entropy-coded scan of
/// [`codec::encode_scan`](crate::codec::encode_scan), for
/// [`codec::decode_scan`](crate::codec::decode_scan) to read. Without a bend
/// (`params` is `None`) it does nothing. With one, the scan's bytes,
/// two to a word from `jpeg_base` on, the low byte first, are written in stream order
/// in bursts of `pitch` words through the address and data faults acting at each
/// burst's write, and read back in the same bursts a frame period later through the
/// cells at the SDRAM's temperature and the faults acting then; see [The JPEG
/// buffer](self#the-jpeg-buffer). The scan keeps its length. It follows [`bend`] of
/// the same frame, whose raw reads its writes' contacts share; without faults or
/// decayed cells the scan comes back unchanged.
///
/// # Examples
///
/// ```
/// use vcb_core::memory::{self, State};
///
/// let mut scan = vec![0x12, 0x34, 0xff, 0x00, 0x56];
/// memory::bend_scan(None, &mut State::default(), 0, &mut scan);
/// assert_eq!(scan, [0x12, 0x34, 0xff, 0x00, 0x56]);
/// ```
pub fn bend_scan(params: Option<&Params>, state: &mut State, index: u64, scan: &mut [u8]) {
    let Some(params) = params else {
        return;
    };
    if scan.is_empty() {
        return;
    }
    state.power_on = params.power_on;
    if let Some(own) = outside(&params.scan_span(scan.len()), &state.sdram.span()) {
        state.scan.cover(&own, &params.power_on);
    }
    state
        .cells
        .update(&params.retention, state.sdram.span(), params.sample_bits);
    state.scan_cells.update(
        &params.retention,
        state.scan.span(),
        params.retention.word_bits,
    );
    let words = (scan.len() as u64).div_ceil(2);
    let bursts = words.div_ceil(u64::from(params.pitch)) as usize;
    state.sample_scan_contacts(params, index, bursts);
    write_scan(params, state, scan);
    read_scan(params, state, scan);
}

/// Returns word `i` of `scan`: bytes `2i` and `2i + 1`, the low byte first, a missing one zero.
fn pack(scan: &[u8], i: usize) -> u16 {
    let high = scan.get(2 * i + 1).copied().unwrap_or(0);
    u16::from_le_bytes([scan[2 * i], high])
}

/// Writes the scan's words in stream order through the faults acting at each burst's write.
///
/// A word lands in the raw's store where that covers its address, boxed, and in the
/// scan's own elsewhere; the later write wins where words fold.
fn write_scan(params: &Params, state: &mut State, scan: &[u8]) {
    let pitch = params.pitch as usize;
    let words = scan.len().div_ceil(2);
    let State {
        sdram,
        scan: store,
        scan_write_active,
        scan_data_write_active,
        ..
    } = state;
    let held = sdram.span();
    for (burst, (&active, &data_active)) in scan_write_active
        .iter()
        .zip(scan_data_write_active.iter())
        .enumerate()
    {
        let decoder = Decoder::new(&params.faults, active);
        let pins = Pins::new(&params.scan_data_faults, data_active);
        let (first, end) = (burst * pitch, ((burst + 1) * pitch).min(words));
        let base = params.jpeg_base + first as u64;
        let range = base..base + (end - first) as u64;
        let own = store.span();
        // A burst through no address fault, wholly in the scan's own store.
        let in_place = decoder.is_identity()
            && (range.end <= held.start || held.end <= range.start)
            && own.start <= range.start
            && range.end <= own.end;
        if in_place {
            let at = store.slot(range.start);
            for (slot, i) in store.words[at..at + (end - first)]
                .iter_mut()
                .zip(first..end)
            {
                *slot = pins.apply(u32::from(pack(scan, i))) as u16;
            }
            continue;
        }
        for (i, address) in (first..end).zip(range) {
            let word = pins.apply(u32::from(pack(scan, i))) as u16;
            let address = decoder.decode(address);
            if held.contains(&address) {
                let at = sdram.slot(address);
                sdram.words[at] = boxed(word);
            } else {
                debug_assert!(
                    store.span().contains(&address),
                    "scan word {i} decodes to {address} outside the stores"
                );
                let at = store.slot(address);
                store.words[at] = word;
            }
        }
    }
}

/// What the read of the scan's words needs.
struct Fetch<'a> {
    sdram: &'a Sdram,
    /// The decayed cells of the raw store's sample bits; `None` while no cell decays.
    masks: Option<&'a [Decay]>,
    store: &'a Words,
    /// The decayed cells of the scan's store, every bit; `None` while no cell decays.
    store_masks: Option<&'a [Decay]>,
    retention: &'a Retention,
    power_on: &'a PowerOn,
}

impl Fetch<'_> {
    /// Returns the 16 bits at `address` and the cells of the word that decay.
    fn word(&self, address: u64) -> (u32, Decay) {
        if self.sdram.span().contains(&address) {
            let at = self.sdram.slot(address);
            let bits = self.power_on.bits_of(self.sdram.words[at]);
            // The raw's masks hold the sample bits; the high bits' cells are hashed here.
            let decay = self.masks.map_or(Decay::default(), |masks| {
                let high = self.retention.sample_bits..self.retention.word_bits;
                masks[at].union(self.retention.decay_bits(address, high))
            });
            (bits, decay)
        } else {
            let at = self.store.slot(address);
            let decay = self.store_masks.map_or(Decay::default(), |masks| masks[at]);
            (u32::from(self.store.words[at]), decay)
        }
    }
}

/// Reads the scan's words back through the cells and the faults acting at each burst's read.
fn read_scan(params: &Params, state: &State, scan: &mut [u8]) {
    let pitch = params.pitch as usize;
    let fetch = Fetch {
        sdram: &state.sdram,
        masks: state.cells.masks(),
        store: &state.scan,
        store_masks: state.scan_cells.masks(),
        retention: &params.retention,
        power_on: &params.power_on,
    };
    scan.par_chunks_mut(2 * pitch)
        .enumerate()
        .for_each(|(burst, bytes)| {
            let decoder = Decoder::new(&params.faults, state.scan_read_active[burst]);
            let pins = Pins::new(&params.scan_data_faults, state.scan_data_read_active[burst]);
            let base = params.jpeg_base + (burst * pitch) as u64;
            for (address, pair) in (base..).zip(bytes.chunks_mut(2)) {
                let (bits, decay) = fetch.word(decoder.decode(address));
                let [low, high, ..] = pins.apply(decay.apply(bits)).to_le_bytes();
                pair[0] = low;
                if let Some(slot) = pair.get_mut(1) {
                    *slot = high;
                }
            }
        });
}

/// Puts every captured row's samples through the data faults acting at its write, in place.
///
/// The raw then holds what its rows store.
fn fault_writes(params: &Params, state: &State, captured: u32, raw: &mut Raw) {
    let max_code = params.power_on.max_code;
    raw.plane_mut()
        .data_mut()
        .par_chunks_mut(params.pitch as usize)
        .enumerate()
        .for_each(|(y, sites)| {
            let line = params.line(y as u32);
            if line >= captured {
                return;
            }
            let pins = Pins::new(&params.data_faults, state.data_write_active[line as usize]);
            if pins.is_identity() {
                return;
            }
            let black = state.black[y];
            for site in sites {
                *site = write_word(*site, black, &pins, max_code);
            }
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

/// What the read of every row needs.
struct Load<'a> {
    params: &'a Params,
    sdram: &'a Sdram,
    /// The decayed cells of the store's words; `None` while no cell decays.
    masks: Option<&'a [Decay]>,
    read_active: &'a [u32],
    data_write_active: &'a [u32],
    data_read_active: &'a [u32],
    /// Each image row's black level; filled while data faults or cells change words.
    black: &'a [f32],
    /// Whether a row read through no address fault is read again from its words.
    reread: bool,
}

/// Reads image row `y` from the SDRAM through the cells and the faults acting at its read.
///
/// A row read through no address fault is read only if `reread`: otherwise it holds
/// what it stored, and passes the cells and the data pins in place.
fn load_row(load: &Load<'_>, y: u32, sites: &mut [f32]) {
    let (params, sdram) = (load.params, load.sdram);
    let line = params.line(y) as usize;
    let first = params.address(0, y);
    let decoder = Decoder::new(&params.faults, load.read_active[line]);
    // A row that holds what it stored crossed the same pins at its write, and a stuck
    // or bridged pin changes nothing the second time unless a decayed cell came between.
    let active = load.data_read_active[line];
    let repeated = decoder.is_identity()
        && !load.reread
        && load.masks.is_none()
        && active == load.data_write_active[line];
    let pins = Pins::new(&params.data_faults, if repeated { 0 } else { active });
    let bits = !pins.is_identity() || load.masks.is_some();
    let black = if bits { load.black[y as usize] } else { 0.0 };
    let power_on = &params.power_on;
    let max_code = power_on.max_code;
    // Without data faults, a word whose cells all hold reads as it is.
    let cells_only = pins.is_identity();
    let read = |value: f32, decay: Decay| {
        if cells_only && decay.is_empty() {
            value
        } else {
            read_word(value, black, decay, &pins, max_code)
        }
    };
    if decoder.is_identity() {
        let (at, len) = (sdram.slot(first), sites.len());
        if load.reread {
            // A word the raw did not write holds 16 bits: the power-on content or the scan's.
            for (site, &word) in sites.iter_mut().zip(&sdram.words[at..at + len]) {
                *site = power_on.sample(word);
            }
        }
        match (bits, load.masks) {
            (false, _) => {}
            (true, Some(masks)) => {
                for (site, &decay) in sites.iter_mut().zip(&masks[at..at + len]) {
                    *site = read(*site, decay);
                }
            }
            (true, None) => {
                for site in sites {
                    *site = read(*site, Decay::default());
                }
            }
        }
        return;
    }
    for (x, site) in (0u64..).zip(sites.iter_mut()) {
        let at = sdram.slot(decoder.decode(first + x));
        let value = power_on.sample(sdram.words[at]);
        *site = if bits {
            read(
                value,
                load.masks.map_or(Decay::default(), |masks| masks[at]),
            )
        } else {
            value
        };
    }
}

#[cfg(test)]
mod tests {
    use super::normal_cdf;

    #[test]
    fn the_normal_distribution_function_keeps_its_relative_accuracy_in_the_tail() {
        // Φ(x) from erfc to 15 digits.
        for (x, expected) in [
            (-15.2, 1.768_105_234_122_279e-52),
            (-6.2, 2.823_158_037_043_268_7e-10),
            (-4.8, 7.933_281_519_755_973e-7),
            (-2.032, 0.021_076_827_973_313_69),
            (-1.0, 0.158_655_253_931_457_07),
            (0.0, 0.5),
            (2.1, 0.982_135_579_437_183_4),
        ] {
            let got = normal_cdf(x);
            assert!(
                (got / expected - 1.0).abs() < 2e-7,
                "Φ({x}) = {got}, expected {expected}"
            );
        }
    }
}
