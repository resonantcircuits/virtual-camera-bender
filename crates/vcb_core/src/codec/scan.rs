//! The entropy-coded scan: MCU order, bit packing, restart markers and the decoder's rules.
//!
//! # Encoding
//!
//! The scan interleaves the three components MCU by MCU, MCUs in raster order: in each
//! MCU the luma blocks left to right and top to bottom (two for 4:2:2, four for 4:2:0,
//! one for 4:4:4), then one Cb and one Cr block (T.81, A.2.3). A block is coded as its
//! DC difference to the previous block of the same component, a size category through
//! the component's DC table and the category's extra bits, then its AC coefficients in
//! zigzag order as `run << 4 | size` symbols with their extra bits, `0xF0` for each
//! run of sixteen zeros before a coefficient and `0x00`, end of block, after the last
//! non-zero one unless it is the 63rd (F.1.2). Negative values carry the low bits of
//! `value - 1`. Bits are packed most significant first, every `0xFF` byte is followed
//! by a stuffed `0x00` (F.1.2.3), and the last byte is padded with one bits. With a
//! restart interval of `n` MCUs, every `n` MCUs but at the end the coder pads to a
//! byte, writes `RSTm` (`0xFF`, `0xD0 + m`, `m` counting modulo 8 from 0) and resets
//! the DC predictions to 0.
//!
//! # Decoding
//!
//! The decoder is libjpeg's (`jdhuff.c` and `jdmarker.c`, the same in libjpeg 6b and
//! libjpeg-turbo), reduced to the bit stream it sees:
//!
//! - The bit reader takes bytes in order; `0xFF` followed by any number of further
//!   `0xFF` and a `0x00` is one data byte `0xFF`; `0xFF` followed by any other byte is
//!   a marker. The reader never reads past a marker: it keeps the marker as unread
//!   and supplies zero bits after the last data bit (`jpeg_fill_bit_buffer`). The end
//!   of the data reads as the marker EOI, as libjpeg's data sources insert one.
//! - Codes are decoded as in [`huffman`](super::huffman): a pattern that matches no
//!   code consumes 17 bits and reads symbol 0, a zero DC difference or an end of block
//!   (`jpeg_huff_decode`).
//! - A DC prediction is a wrapping 32-bit sum; the block keeps its low 16 bits.
//! - An AC run past the block's last coefficient writes the coefficient to the
//!   last position, 63, and ends the block; a zero run past it ends the block
//!   (`jpeg_natural_order`'s 16 extra entries, all 63).
//! - Once a decoded MCU has consumed one of the zero bits after a marker, every
//!   following MCU is left all zero (`insufficient_data`), until a restart clears it.
//! - Before every `restart_interval`-th MCU, when the interval is non-zero, the
//!   decoder drops the bits it holds, finds the next marker (skipping data bytes) if
//!   it has none unread, and compares it with the expected `RSTm`. The expected one is
//!   consumed. Any other goes to libjpeg's `jpeg_resync_to_restart`: a code below
//!   `0xC0`, no JPEG marker, is skipped and the next marker examined; a non-restart
//!   marker, or `RST(m + 1)` or `RST(m + 2)`, is left unread, so this interval decodes
//!   from zero bits; `RST(m - 1)` or `RST(m - 2)` is skipped for the next marker; any
//!   other restart marker is consumed as if it were the expected one. Then the DC
//!   predictions are reset, the expected number advances, and the all-zero state is
//!   cleared if no marker is left unread.
//!
//! libjpeg's own buffering (how far ahead it reads) is not observable: it never reads
//! past a marker, and the bits it drops at a restart are data that the search for the
//! next marker would skip.

use super::huffman::{self, Decoder, Encoder, Pair};

/// The 64 quantised coefficients of a block in natural order, `[8 * v + u]`.
pub(super) type Coefficients = [i16; 64];

/// The natural index of each zigzag position (T.81 Figure A.6), then 16 times 63.
///
/// The 16 extra entries are libjpeg's: a corrupted run that passes the last position
/// writes its coefficient there instead of beyond the block.
const NATURAL: [usize; 80] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63, 63,
];

/// The AC symbol for the end of a block.
const EOB: u8 = 0x00;

/// The AC symbol for a run of sixteen zeros.
const ZRL: u8 = 0xF0;

/// The largest DC difference baseline JPEG codes, of size category 11.
const MAX_DC_DIFFERENCE: i32 = 2047;

/// The largest AC magnitude baseline JPEG codes, of size category 10.
const MAX_AC: i32 = 1023;

/// The marker byte of `RST0`; `RSTm` is `RST0 + m`.
const RST0: u8 = 0xD0;

/// The marker byte of EOI, which libjpeg's data sources report past the end of the data.
const EOI: u8 = 0xD9;

/// The marker byte of SOF0; libjpeg treats codes below it as no marker at all.
const SOF0: u8 = 0xC0;

/// Bits the reader holds before each code, enough for a code and its extra bits.
///
/// The longest step is a 16-bit AC code with 10 extra bits, or a 17-bit bad code.
const REFILL_BITS: i32 = 32;

/// The MCU grid of an interleaved scan of a luma and two chroma components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Grid {
    /// MCUs across the frame.
    pub(super) columns: usize,
    /// MCUs down the frame.
    pub(super) rows: usize,
    /// Luma blocks per MCU across and down; each chroma component has one.
    pub(super) factors: (usize, usize),
}

impl Grid {
    /// Calls `f(component, block)` for every block of MCU `(column, row)` in scan order.
    ///
    /// Components are 0 for luma, 1 for Cb and 2 for Cr; blocks are indices in the
    /// component's blocks in raster order.
    #[inline]
    fn for_each_block(&self, column: usize, row: usize, mut f: impl FnMut(usize, usize)) {
        let (sx, sy) = self.factors;
        let luma_columns = self.columns * sx;
        for dy in 0..sy {
            for dx in 0..sx {
                f(0, (row * sy + dy) * luma_columns + column * sx + dx);
            }
        }
        let chroma = row * self.columns + column;
        f(1, chroma);
        f(2, chroma);
    }

    /// Returns the number of blocks of each component.
    fn blocks(&self) -> [usize; 3] {
        let chroma = self.columns * self.rows;
        [chroma * self.factors.0 * self.factors.1, chroma, chroma]
    }
}

/// Writes the scan of the quantised `blocks` of luma, Cb and Cr into `scan`.
///
/// `scan` is cleared first. With a non-zero `restart_interval` an `RSTm` marker follows
/// every `restart_interval` MCUs but the last.
pub(super) fn encode(
    grid: Grid,
    blocks: [&[Coefficients]; 3],
    restart_interval: u32,
    scan: &mut Vec<u8>,
) {
    check(grid, blocks.map(<[Coefficients]>::len));
    scan.clear();
    let tables = [
        &huffman::LUMA_ENCODER,
        &huffman::CHROMA_ENCODER,
        &huffman::CHROMA_ENCODER,
    ];
    let interval = restart_interval as usize;
    let mut writer = Writer::new(scan);
    let mut predictions = [0i32; 3];
    let mut restart = 0u8;
    let mut mcu = 0usize;
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            if interval != 0 && mcu != 0 && mcu.is_multiple_of(interval) {
                writer.align();
                writer.marker(RST0 + restart);
                restart = (restart + 1) & 7;
                predictions = [0; 3];
            }
            grid.for_each_block(column, row, |c, i| {
                encode_block(&mut writer, tables[c], &mut predictions[c], &blocks[c][i]);
            });
            mcu += 1;
        }
    }
    writer.align();
}

/// Decodes `scan` into the quantised `blocks` of luma, Cb and Cr as libjpeg would.
///
/// Every block is overwritten; the blocks of MCUs the decoder skips are all zero.
pub(super) fn decode(
    grid: Grid,
    scan: &[u8],
    restart_interval: u32,
    mut blocks: [&mut [Coefficients]; 3],
) {
    check(grid, blocks.each_ref().map(|b| b.len()));
    for component in &mut blocks {
        component.fill([0; 64]);
    }
    let tables = [
        &huffman::LUMA_DECODER,
        &huffman::CHROMA_DECODER,
        &huffman::CHROMA_DECODER,
    ];
    let mut reader = Reader::new(scan);
    let mut predictions = [0i32; 3];
    let mut insufficient = false;
    let mut restarts_to_go = restart_interval;
    let mut expected = 0u8;
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            if restart_interval != 0 {
                if restarts_to_go == 0 {
                    reader.restart(expected);
                    expected = (expected + 1) & 7;
                    predictions = [0; 3];
                    restarts_to_go = restart_interval;
                    if reader.marker.is_none() {
                        insufficient = false;
                    }
                }
                restarts_to_go -= 1;
            }
            if insufficient {
                continue;
            }
            grid.for_each_block(column, row, |c, i| {
                decode_block(
                    &mut reader,
                    tables[c],
                    &mut predictions[c],
                    &mut blocks[c][i],
                );
            });
            insufficient = reader.overran();
        }
    }
}

/// Panics unless each component holds the blocks `grid` needs.
fn check(grid: Grid, lengths: [usize; 3]) {
    assert_eq!(
        lengths,
        grid.blocks(),
        "the components hold {lengths:?} blocks but the MCU grid {grid:?} needs {:?}",
        grid.blocks()
    );
}

/// Returns the size category of `value` and its extra bits.
#[inline]
fn category(value: i32) -> (u32, u32) {
    let size = 32 - value.unsigned_abs().leading_zeros();
    let bits = if value < 0 { value - 1 } else { value } as u32;
    (size, bits & ((1 << size) - 1))
}

/// Codes one block, updating its component's DC prediction.
///
/// Coefficients beyond baseline's range are clamped to it, so the scan stays
/// decodable; quantised 8-bit samples never reach that range.
#[inline]
fn encode_block(
    writer: &mut Writer<'_>,
    tables: &Pair<Encoder>,
    prediction: &mut i32,
    block: &Coefficients,
) {
    let difference =
        (i32::from(block[0]) - *prediction).clamp(-MAX_DC_DIFFERENCE, MAX_DC_DIFFERENCE);
    *prediction += difference;
    let (size, bits) = category(difference);
    let (code, length) = tables.dc.code(size as u8);
    writer.put(code << size | bits, length + size);

    // The AC coefficients in zigzag order and a mask of the non-zero ones, so that the
    // coder visits only those: on noisy blocks a test per coefficient mispredicts.
    let zigzag: [i16; 64] = std::array::from_fn(|k| block[NATURAL[k]]);
    let nonzero = zigzag
        .iter()
        .enumerate()
        .skip(1)
        .fold(0u64, |mask, (k, &c)| mask | u64::from(c != 0) << k);
    let mut remaining = nonzero;
    let mut last = 0;
    while remaining != 0 {
        let k = remaining.trailing_zeros();
        remaining &= remaining - 1;
        let mut run = k - last - 1;
        while run > 15 {
            let (code, length) = tables.ac.code(ZRL);
            writer.put(code, length);
            run -= 16;
        }
        let value = i32::from(zigzag[k as usize]).clamp(-MAX_AC, MAX_AC);
        let (size, bits) = category(value);
        let (code, length) = tables.ac.code((run << 4 | size) as u8);
        writer.put(code << size | bits, length + size);
        last = k;
    }
    if last != 63 {
        let (code, length) = tables.ac.code(EOB);
        writer.put(code, length);
    }
}

/// Decodes one block into `block`, which is all zero on entry.
#[inline]
fn decode_block(
    reader: &mut Reader<'_>,
    tables: &Pair<Decoder>,
    prediction: &mut i32,
    block: &mut Coefficients,
) {
    reader.refill();
    let size = reader.symbol(&tables.dc);
    let difference = reader.extend(u32::from(size));
    *prediction = prediction.wrapping_add(difference);
    // libjpeg stores the sum in a 16-bit coefficient, keeping its low bits.
    block[0] = *prediction as i16;

    let mut k = 1;
    while k < 64 {
        reader.refill();
        let symbol = reader.symbol(&tables.ac);
        let (run, size) = (usize::from(symbol >> 4), u32::from(symbol & 15));
        if size == 0 {
            if run != 15 {
                break;
            }
            k += 16;
            continue;
        }
        k += run;
        block[NATURAL[k]] = reader.extend(size) as i16;
        k += 1;
    }
}

/// Returns whether any byte of `word` is `0xFF`.
#[inline]
fn has_ff(word: u64) -> bool {
    // A byte of `word` is 0xFF exactly where a byte of `!word` is zero.
    let inverted = !word;
    inverted.wrapping_sub(0x0101_0101_0101_0101) & !inverted & 0x8080_8080_8080_8080 != 0
}

/// Packs codes into bytes with byte stuffing.
struct Writer<'a> {
    out: &'a mut Vec<u8>,
    /// Pending bits, the newest least significant; only the low `count` are valid.
    bits: u64,
    /// Number of pending bits, below 32 between calls.
    count: u32,
}

impl<'a> Writer<'a> {
    fn new(out: &'a mut Vec<u8>) -> Self {
        Self {
            out,
            bits: 0,
            count: 0,
        }
    }

    /// Appends the low `length` bits of `code`, at most 32, most significant first.
    #[inline]
    fn put(&mut self, code: u32, length: u32) {
        self.bits = self.bits << length | u64::from(code);
        self.count += length;
        if self.count >= 32 {
            self.count -= 32;
            self.word((self.bits >> self.count) as u32);
        }
    }

    /// Writes four bytes, stuffing a zero after each `0xFF`.
    #[inline]
    fn word(&mut self, word: u32) {
        if has_ff(u64::from(word)) {
            for byte in word.to_be_bytes() {
                self.byte(byte);
            }
        } else {
            self.out.extend_from_slice(&word.to_be_bytes());
        }
    }

    /// Writes one byte, stuffing a zero after `0xFF`.
    fn byte(&mut self, byte: u8) {
        self.out.push(byte);
        if byte == 0xFF {
            self.out.push(0);
        }
    }

    /// Pads the pending bits with ones to a whole byte and writes them out.
    fn align(&mut self) {
        let pad = (8 - self.count % 8) % 8;
        self.bits = self.bits << pad | ((1 << pad) - 1);
        self.count += pad;
        while self.count > 0 {
            self.count -= 8;
            self.byte((self.bits >> self.count) as u8);
        }
    }

    /// Writes a marker; the bits must be aligned.
    fn marker(&mut self, code: u8) {
        self.out.extend_from_slice(&[0xFF, code]);
    }
}

/// What libjpeg's `jpeg_resync_to_restart` does with an unexpected marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resync {
    /// Consume the marker and decode the data after it (libjpeg's action 1).
    Discard,
    /// Skip to the next marker and decide again (action 2).
    Advance,
    /// Leave the marker unread: the interval decodes from zero bits (action 3).
    Keep,
}

impl Resync {
    /// Chooses the action for `marker` when `RST(expected)` was due.
    fn choose(marker: u8, expected: u8) -> Self {
        if marker < SOF0 {
            return Self::Advance;
        }
        if !(RST0..=RST0 + 7).contains(&marker) {
            return Self::Keep;
        }
        match (marker - RST0).wrapping_sub(expected) & 7 {
            1 | 2 => Self::Keep,
            6 | 7 => Self::Advance,
            _ => Self::Discard,
        }
    }
}

/// libjpeg's bit reader over one scan.
struct Reader<'a> {
    data: &'a [u8],
    /// The next byte of `data` to read.
    position: usize,
    /// The next bits, most significant first; zero below the valid ones.
    window: u64,
    /// Data bits in `window` not yet consumed; negative once the decoder has consumed
    /// zero bits that follow a marker.
    bits: i32,
    /// The marker that stopped the reader, libjpeg's `unread_marker`.
    marker: Option<u8>,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            position: 0,
            window: 0,
            bits: 0,
            marker: None,
        }
    }

    /// Returns whether the decoder has consumed bits beyond the data before a marker.
    fn overran(&self) -> bool {
        self.bits < 0
    }

    /// Tops the window up to at least [`REFILL_BITS`] bits, zeros after a marker.
    #[inline]
    fn refill(&mut self) {
        if self.bits < REFILL_BITS && self.marker.is_none() {
            self.fill();
        }
    }

    fn fill(&mut self) {
        // Eight bytes without an 0xFF hold neither a marker nor a stuffed byte: take as
        // many whole bytes of them as the window has room for at once.
        if let Some(chunk) = self.data.get(self.position..self.position + 8) {
            let word = u64::from_be_bytes(chunk.try_into().unwrap_or([0xFF; 8]));
            if !has_ff(word) {
                let room = (64 - self.bits as u32) / 8;
                self.window |= word >> (64 - 8 * room) << (64 - self.bits as u32 - 8 * room);
                self.position += room as usize;
                self.bits += (8 * room).cast_signed();
                return;
            }
        }
        while self.bits <= 56 {
            let Some(byte) = self.data_byte() else {
                return;
            };
            self.window |= u64::from(byte) << (56 - self.bits);
            self.bits += 8;
        }
    }

    /// Returns the next data byte, or `None` at a marker, which it records.
    fn data_byte(&mut self) -> Option<u8> {
        match self.next_byte() {
            Some(0xFF) => loop {
                match self.next_byte() {
                    Some(0xFF) => {}
                    Some(0) => return Some(0xFF),
                    code => {
                        self.marker = Some(code.unwrap_or(EOI));
                        return None;
                    }
                }
            },
            None => {
                self.marker = Some(EOI);
                None
            }
            byte => byte,
        }
    }

    fn next_byte(&mut self) -> Option<u8> {
        let byte = self.data.get(self.position).copied()?;
        self.position += 1;
        Some(byte)
    }

    #[inline]
    fn consume(&mut self, count: u32) {
        self.window <<= count;
        self.bits -= count.cast_signed();
    }

    /// Decodes one Huffman code with `table`.
    #[inline]
    fn symbol(&mut self, table: &Decoder) -> u8 {
        let (symbol, length) = table.decode(self.window);
        self.consume(length);
        symbol
    }

    /// Reads `size` extra bits and returns the value they code in category `size`.
    #[inline]
    fn extend(&mut self, size: u32) -> i32 {
        if size == 0 {
            return 0;
        }
        let bits = (self.window >> (64 - size)) as i32;
        self.consume(size);
        if bits < 1 << (size - 1) {
            bits - (1 << size) + 1
        } else {
            bits
        }
    }

    /// Processes the restart due before an interval: libjpeg's `process_restart`.
    fn restart(&mut self, expected: u8) {
        self.window = 0;
        self.bits = 0;
        if self.marker.is_none() {
            self.next_marker();
        }
        if self.marker == Some(RST0 + expected) {
            self.marker = None;
        } else {
            self.resync(expected);
        }
    }

    /// libjpeg's `jpeg_resync_to_restart` with the reader at an unexpected marker.
    fn resync(&mut self, expected: u8) {
        while let Some(marker) = self.marker {
            match Resync::choose(marker, expected) {
                Resync::Discard => self.marker = None,
                Resync::Advance => self.next_marker(),
                Resync::Keep => return,
            }
        }
    }

    /// Skips data bytes to the next marker and records it: libjpeg's `next_marker`.
    fn next_marker(&mut self) {
        loop {
            let Some(offset) = self.data[self.position..].iter().position(|&b| b == 0xFF) else {
                self.position = self.data.len();
                self.marker = Some(EOI);
                return;
            };
            self.position += offset + 1;
            let code = loop {
                match self.next_byte() {
                    Some(0xFF) => {}
                    Some(code) => break code,
                    None => {
                        self.marker = Some(EOI);
                        return;
                    }
                }
            };
            if code != 0 {
                self.marker = Some(code);
                return;
            }
        }
    }
}
