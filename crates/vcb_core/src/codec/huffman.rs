//! The Huffman tables of baseline JPEG: the standard's example tables and their codes.
//!
//! A table is specified as the standard's `BITS` and `HUFFVAL` lists (ITU-T T.81,
//! B.2.4.2): the number of codes of each length from 1 to 16 bits, and the symbols in
//! order of increasing code length. The four tables here are those of Annex K.3
//! (Tables K.3 to K.6), luma and chroma, DC and AC, which libjpeg installs by default
//! and most cameras of the A520's time used.
//!
//! The codes follow from the lists by Annex C: code lengths in symbol order (Figure
//! C.1), canonical codes counting up within a length and doubling at each longer
//! length (Figure C.2), and the encoder's table indexed by symbol (Figure C.3). The
//! decoder's tables are those of Figure F.15, the largest code of each length and the
//! offset from a code to its symbol, plus a lookahead table that resolves every code
//! of up to [`LOOKAHEAD`] bits in one step; longer codes are resolved bit by bit as
//! in Figure F.16. All of it is computed at compile time.
//!
//! A bit pattern that is no code at all (the standard's tables leave the all-ones
//! patterns unused) decodes as libjpeg's `jpeg_huff_decode` decodes it: the search
//! runs past 16 bits to the sentinel at length 17, consumes the 17 bits it examined,
//! and returns symbol 0.

/// Bits the decoder's lookahead table resolves in one step.
///
/// Nine bits resolve every luma DC code and the short AC codes the standard gives
/// the commonest symbols; the table has `2^9` entries per Huffman table.
pub(super) const LOOKAHEAD: u32 = 9;

/// The code length past the longest legal code at which a bad code is given up.
///
/// libjpeg's decoder keeps `maxcode[17]` as a sentinel larger than any 17-bit code, so
/// a pattern that matches no code of up to 16 bits is read to 17 bits and abandoned.
pub(super) const BAD_CODE_BITS: u32 = 17;

/// The sentinel of `maxcode` at length 17, libjpeg's `0xFFFFF`.
const SENTINEL: i32 = 0xF_FFFF;

/// A Huffman table as the standard lists it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Spec {
    /// `BITS`: the number of codes of each length, 1 to 16 bits.
    counts: [u8; 16],
    /// `HUFFVAL`: the symbols in order of increasing code length.
    symbols: &'static [u8],
}

/// Table K.3: luma DC differences, size categories 0 to 11.
pub(super) const LUMA_DC: Spec = Spec {
    counts: [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    symbols: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

/// Table K.4: chroma DC differences, size categories 0 to 11.
pub(super) const CHROMA_DC: Spec = Spec {
    counts: [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
    symbols: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

/// Table K.5: luma AC coefficients, symbols `run << 4 | size`.
pub(super) const LUMA_AC: Spec = Spec {
    counts: [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d],
    symbols: &[
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61,
        0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52,
        0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25,
        0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45,
        0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64,
        0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83,
        0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
        0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6,
        0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3,
        0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8,
        0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
    ],
};

/// Table K.6: chroma AC coefficients, symbols `run << 4 | size`.
pub(super) const CHROMA_AC: Spec = Spec {
    counts: [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
    symbols: &[
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61,
        0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33,
        0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18,
        0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44,
        0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63,
        0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a,
        0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
        0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4,
        0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca,
        0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7,
        0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
    ],
};

/// Code lengths and codes in `HUFFVAL` order (Figures C.1 and C.2).
struct Canonical {
    /// `HUFFSIZE`: the length of the code of each listed symbol.
    lengths: [u8; 256],
    /// `HUFFCODE`: the code of each listed symbol, right-aligned.
    codes: [u16; 256],
    /// The number of listed symbols, `LASTK`.
    count: usize,
}

impl Canonical {
    /// Generates the canonical codes of `spec`.
    ///
    /// # Panics
    ///
    /// Panics (at compile time for the tables here) if the lists disagree on the
    /// number of symbols or a length overflows its code space.
    const fn new(spec: &Spec) -> Self {
        let mut lengths = [0u8; 256];
        let mut count = 0;
        let mut length = 1;
        while length <= 16 {
            let mut i = 0;
            while i < spec.counts[length - 1] {
                lengths[count] = length as u8;
                count += 1;
                i += 1;
            }
            length += 1;
        }
        assert!(count == spec.symbols.len(), "BITS and HUFFVAL disagree");

        let mut codes = [0u16; 256];
        let mut code = 0u32;
        let mut size = lengths[0];
        let mut k = 0;
        while k < count {
            while k < count && lengths[k] == size {
                codes[k] = code as u16;
                code += 1;
                k += 1;
            }
            assert!(code <= 1 << size, "a code length overflows its code space");
            while k < count && lengths[k] != size {
                code <<= 1;
                size += 1;
            }
        }
        Self {
            lengths,
            codes,
            count,
        }
    }
}

/// The encoder's table: the code of every symbol (Figure C.3).
#[derive(Debug, Clone, Copy)]
pub(super) struct Encoder {
    /// `EHUFCO`: the code of each symbol, right-aligned.
    codes: [u16; 256],
    /// `EHUFSI`: the length of each symbol's code; 0 for a symbol not in the table.
    lengths: [u8; 256],
}

impl Encoder {
    /// Builds the encoder's table of `spec`.
    pub(super) const fn new(spec: &Spec) -> Self {
        let generated = Canonical::new(spec);
        let mut codes = [0u16; 256];
        let mut lengths = [0u8; 256];
        let mut k = 0;
        while k < generated.count {
            let symbol = spec.symbols[k] as usize;
            codes[symbol] = generated.codes[k];
            lengths[symbol] = generated.lengths[k];
            k += 1;
        }
        Self { codes, lengths }
    }

    /// Returns the code of `symbol` and its length in bits.
    #[inline]
    pub(super) fn code(&self, symbol: u8) -> (u32, u32) {
        let i = usize::from(symbol);
        (u32::from(self.codes[i]), u32::from(self.lengths[i]))
    }
}

/// The decoder's tables (Figure F.15) and a lookahead table.
#[derive(Debug, Clone, Copy)]
pub(super) struct Decoder {
    /// Every `LOOKAHEAD`-bit window: `length << 8 | symbol` of the code it starts with,
    /// or 0 if that code is longer.
    lookahead: [u16; 1 << LOOKAHEAD],
    /// `MAXCODE`: the largest code of each length, -1 if none; index 17 is the sentinel.
    max_codes: [i32; 18],
    /// The index in `symbols` of each length's first code, minus that code.
    offsets: [i32; 18],
    /// `HUFFVAL`, padded with zeros.
    symbols: [u8; 256],
}

impl Decoder {
    /// Builds the decoder's tables of `spec`.
    pub(super) const fn new(spec: &Spec) -> Self {
        let generated = Canonical::new(spec);
        let mut max_codes = [-1i32; 18];
        let mut offsets = [0i32; 18];
        let mut first = 0;
        let mut length = 1;
        while length <= 16 {
            let n = spec.counts[length - 1] as usize;
            if n > 0 {
                offsets[length] = first as u16 as i32 - generated.codes[first] as i32;
                max_codes[length] = generated.codes[first + n - 1] as i32;
                first += n;
            }
            length += 1;
        }
        max_codes[17] = SENTINEL;

        let mut lookahead = [0u16; 1 << LOOKAHEAD];
        let mut k = 0;
        while k < generated.count {
            let size = generated.lengths[k] as u32;
            if size <= LOOKAHEAD {
                let start = (generated.codes[k] as usize) << (LOOKAHEAD - size);
                let mut i = 0;
                while i < 1 << (LOOKAHEAD - size) {
                    lookahead[start + i] = (size as u16) << 8 | spec.symbols[k] as u16;
                    i += 1;
                }
            }
            k += 1;
        }

        let mut symbols = [0u8; 256];
        let mut i = 0;
        while i < spec.symbols.len() {
            symbols[i] = spec.symbols[i];
            i += 1;
        }
        Self {
            lookahead,
            max_codes,
            offsets,
            symbols,
        }
    }

    /// Decodes the code at the top of `window`, returning its symbol and length in bits.
    ///
    /// `window` holds the next bits of the scan, most significant first; at least
    /// [`BAD_CODE_BITS`] of them must be valid. A window that starts with no code
    /// returns symbol 0 and a length of [`BAD_CODE_BITS`].
    #[inline]
    pub(super) fn decode(&self, window: u64) -> (u8, u32) {
        let entry = self.lookahead[(window >> (64 - LOOKAHEAD)) as usize];
        if entry != 0 {
            return ((entry & 0xFF) as u8, u32::from(entry >> 8));
        }
        // Figure F.16 from the first length the lookahead could not resolve.
        let mut length = LOOKAHEAD as usize + 1;
        let mut code = (window >> (64 - length)) as i32;
        while code > self.max_codes[length] {
            length += 1;
            code = (window >> (64 - length)) as i32;
        }
        if length as u32 >= BAD_CODE_BITS {
            return (0, BAD_CODE_BITS);
        }
        let index = (code + self.offsets[length]) as usize;
        (self.symbols[index], length as u32)
    }
}

/// The DC and AC tables of one component.
#[derive(Debug, Clone, Copy)]
pub(super) struct Pair<T> {
    /// The table of DC differences.
    pub(super) dc: T,
    /// The table of AC coefficients.
    pub(super) ac: T,
}

/// The encoder's luma tables.
pub(super) const LUMA_ENCODER: Pair<Encoder> = Pair {
    dc: Encoder::new(&LUMA_DC),
    ac: Encoder::new(&LUMA_AC),
};

/// The encoder's chroma tables.
pub(super) const CHROMA_ENCODER: Pair<Encoder> = Pair {
    dc: Encoder::new(&CHROMA_DC),
    ac: Encoder::new(&CHROMA_AC),
};

/// The decoder's luma tables.
pub(super) const LUMA_DECODER: Pair<Decoder> = Pair {
    dc: Decoder::new(&LUMA_DC),
    ac: Decoder::new(&LUMA_AC),
};

/// The decoder's chroma tables.
pub(super) const CHROMA_DECODER: Pair<Decoder> = Pair {
    dc: Decoder::new(&CHROMA_DC),
    ac: Decoder::new(&CHROMA_AC),
};

#[cfg(test)]
mod tests {
    use super::{BAD_CODE_BITS, CHROMA_AC, CHROMA_DC, Decoder, Encoder, LUMA_AC, LUMA_DC, Spec};

    /// The codes of `spec` as bit strings, by symbol.
    fn strings(spec: &Spec) -> Vec<(u8, String)> {
        let encoder = Encoder::new(spec);
        spec.symbols
            .iter()
            .map(|&s| {
                let (code, len) = encoder.code(s);
                (s, format!("{code:0width$b}", width = len as usize))
            })
            .collect()
    }

    #[test]
    fn ac_tables_hold_every_run_and_size_with_end_of_block_and_zero_run() {
        for spec in [LUMA_AC, CHROMA_AC] {
            let mut symbols = spec.symbols.to_vec();
            symbols.sort_unstable();
            let mut expected: Vec<u8> = (0..16u8)
                .flat_map(|run| (1..=10u8).map(move |size| run << 4 | size))
                .chain([0x00, 0xF0])
                .collect();
            expected.sort_unstable();
            assert_eq!(symbols, expected);
        }
        for spec in [LUMA_DC, CHROMA_DC] {
            assert_eq!(spec.symbols, (0..=11).collect::<Vec<u8>>());
        }
    }

    #[test]
    fn codes_are_prefix_free_and_leave_only_all_ones_patterns_unused() {
        for spec in [LUMA_DC, CHROMA_DC, LUMA_AC, CHROMA_AC] {
            let codes = strings(&spec);
            for (i, (_, a)) in codes.iter().enumerate() {
                assert!(!a.chars().all(|c| c == '1'), "all-ones code {a}");
                for (_, b) in &codes[i + 1..] {
                    assert!(!b.starts_with(a.as_str()) && !a.starts_with(b.as_str()));
                }
            }
            // Kraft: the codes fill the code space but for the one all-ones pattern
            // of the longest length.
            let longest = codes.iter().map(|(_, c)| c.len()).max().unwrap();
            let used: u64 = codes.iter().map(|(_, c)| 1u64 << (longest - c.len())).sum();
            assert_eq!(used, (1u64 << longest) - 1);
        }
    }

    #[test]
    fn generated_codes_match_the_printed_annex_k_tables() {
        // A few entries of Tables K.3 and K.5 as the standard prints them.
        let luma_dc = strings(&LUMA_DC);
        assert_eq!(luma_dc[0].1, "00");
        assert_eq!(luma_dc[1].1, "010");
        assert_eq!(luma_dc[11].1, "111111110");
        let luma_ac: std::collections::HashMap<u8, String> =
            strings(&LUMA_AC).into_iter().collect();
        assert_eq!(luma_ac[&0x00], "1010");
        assert_eq!(luma_ac[&0x01], "00");
        assert_eq!(luma_ac[&0xF0], "11111111001");
        assert_eq!(luma_ac[&0xFA], "1111111111111110");
        let chroma_ac: std::collections::HashMap<u8, String> =
            strings(&CHROMA_AC).into_iter().collect();
        assert_eq!(chroma_ac[&0x00], "00");
        assert_eq!(chroma_ac[&0xF0], "1111111010");
    }

    #[test]
    fn the_decoder_inverts_the_encoder_and_reads_bad_codes_as_libjpeg() {
        for spec in [LUMA_DC, CHROMA_DC, LUMA_AC, CHROMA_AC] {
            let (encoder, decoder) = (Encoder::new(&spec), Decoder::new(&spec));
            for &symbol in spec.symbols {
                let (code, len) = encoder.code(symbol);
                // The code followed by ones, then by zeros: the tail must not matter.
                for tail in [u64::MAX, 0] {
                    let window = (u64::from(code) << (64 - len)) | (tail >> len);
                    assert_eq!(decoder.decode(window), (symbol, len));
                }
            }
            // Sixteen ones and more match nothing: symbol 0, seventeen bits consumed.
            assert_eq!(decoder.decode(u64::MAX), (0, BAD_CODE_BITS));
        }
        // The luma DC table's codes end at nine bits: nine ones are already bad.
        let decoder = Decoder::new(&LUMA_DC);
        assert_eq!(decoder.decode(0xFF80 << 48), (0, BAD_CODE_BITS));
    }
}
