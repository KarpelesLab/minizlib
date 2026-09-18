//! The deflate decoder (RFC 1951).
//!
//! Huffman codes are decoded canonically, one bit at a time, from a count of
//! codes per length and a list of symbols sorted by code, in the manner of
//! zlib's `puff`. This is slower than table lookups but needs very little
//! code and only about 1 KiB of stack.

use crate::{Checksum, Error, Input, Output};

#[cfg(any(feature = "fixed", feature = "dynamic"))]
const MAX_BITS: usize = 15;
/// Most literal/length symbols a block can define.
#[cfg(any(feature = "fixed", feature = "dynamic"))]
const MAX_LEN_SYMS: usize = 288;
/// Most distance symbols a block can define.
#[cfg(any(feature = "fixed", feature = "dynamic"))]
const MAX_DIST_SYMS: usize = 32;
#[cfg(any(feature = "fixed", feature = "dynamic"))]
const MAX_SYMS: usize = MAX_LEN_SYMS + MAX_DIST_SYMS;

/// Decodes one raw deflate stream and flushes the output.
pub(crate) fn inflate<I: Input, O: Output, C: Checksum>(
    input: &mut I,
    out: &mut O,
    check: &mut C,
) -> Result<(), Error> {
    let mut state = Inflate {
        input,
        out,
        check,
        bit_buf: 0,
        bit_cnt: 0,
    };
    loop {
        let last = state.bits(1)? != 0;
        match state.bits(2)? {
            #[cfg(feature = "stored")]
            0 => state.stored()?,
            #[cfg(feature = "fixed")]
            1 => state.fixed()?,
            #[cfg(feature = "dynamic")]
            2 => state.dynamic()?,
            3 => return Err(Error::InvalidBlock),
            _ => return Err(Error::Unsupported),
        }
        if last {
            break;
        }
    }
    // Input is only ever fetched a byte at a time, so the bits left over here
    // are padding and nothing past the end of the stream was consumed.
    out.flush(check)
}

struct Inflate<'a, I, O, C> {
    input: &'a mut I,
    out: &'a mut O,
    check: &'a mut C,
    bit_buf: u32,
    bit_cnt: u32,
}

/// A canonical Huffman code: `count[n]` codes of `n` bits each, and the
/// symbols they decode to, ordered by code.
#[cfg(any(feature = "fixed", feature = "dynamic"))]
struct Huffman<'a> {
    count: [u16; MAX_BITS + 1],
    symbol: &'a mut [u16],
}

#[cfg(any(feature = "fixed", feature = "dynamic"))]
impl Huffman<'_> {
    /// Builds the code from the code length of each symbol. Returns the number
    /// of unused codes: zero for a complete code, negative if over-subscribed.
    fn build(&mut self, lengths: &[u8]) -> i32 {
        self.count = [0; MAX_BITS + 1];
        for &len in lengths {
            self.count[(len & 15) as usize] += 1;
        }

        let mut left = 1;
        let mut offsets = [0u16; MAX_BITS + 1];
        for len in 1..=MAX_BITS {
            left = (left << 1) - self.count[len] as i32;
            if left < 0 {
                return left;
            }
            if len < MAX_BITS {
                offsets[len + 1] = offsets[len] + self.count[len];
            }
        }

        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let offset = &mut offsets[(len & 15) as usize];
                if let Some(slot) = self.symbol.get_mut(*offset as usize) {
                    *slot = sym as u16;
                }
                *offset += 1;
            }
        }
        left
    }

    /// Whether the code, given its `build` result, is one deflate permits:
    /// complete, or made of a single one-bit code.
    #[cfg(feature = "dynamic")]
    fn is_valid(&self, left: i32, symbols: usize) -> bool {
        left == 0 || (left > 0 && symbols == (self.count[0] + self.count[1]) as usize)
    }
}

impl<I: Input, O: Output, C: Checksum> Inflate<'_, I, O, C> {
    /// Reads `need` bits, at most 16, least significant first.
    fn bits(&mut self, need: u32) -> Result<u32, Error> {
        while self.bit_cnt < need {
            self.bit_buf |= (self.input.byte()? as u32) << self.bit_cnt;
            self.bit_cnt += 8;
        }
        let value = self.bit_buf & ((1 << need) - 1);
        self.bit_buf >>= need;
        self.bit_cnt -= need;
        Ok(value)
    }

    #[cfg(feature = "stored")]
    fn stored(&mut self) -> Result<(), Error> {
        // Skip to the next byte boundary.
        self.bit_buf = 0;
        self.bit_cnt = 0;
        let len = self.bits(16)?;
        if self.bits(16)? != len ^ 0xffff {
            return Err(Error::InvalidBlock);
        }
        for _ in 0..len {
            let byte = self.input.byte()?;
            self.out.put(byte, self.check)?;
        }
        Ok(())
    }

    #[cfg(feature = "fixed")]
    fn fixed(&mut self) -> Result<(), Error> {
        let mut lengths = [8u8; MAX_SYMS];
        lengths[144..256].fill(9);
        lengths[256..280].fill(7);
        // All 32 five-bit distance codes, to make the code complete. The last
        // two are rejected when they come up.
        lengths[MAX_LEN_SYMS..].fill(5);
        self.compressed(&lengths, MAX_LEN_SYMS).map(drop)
    }

    #[cfg(feature = "dynamic")]
    fn dynamic(&mut self) -> Result<(), Error> {
        /// Order in which the code length code lengths are stored.
        const ORDER: [u8; 19] = [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ];

        let len_syms = self.bits(5)? as usize + 257;
        let dist_syms = self.bits(5)? as usize + 1;
        let code_syms = self.bits(4)? as usize + 4;
        if len_syms > 286 || dist_syms > 30 {
            return Err(Error::InvalidBlock);
        }

        let mut lengths = [0u8; MAX_SYMS];
        for &sym in ORDER.iter().take(code_syms) {
            lengths[(sym & 31) as usize] = self.bits(3)? as u8;
        }
        let mut symbol = [0; 19];
        let mut code = Huffman {
            count: [0; MAX_BITS + 1],
            symbol: &mut symbol,
        };
        if code.build(&lengths[..19]) != 0 {
            return Err(Error::InvalidCode);
        }

        // Literal/length code lengths, directly followed by the distance ones.
        let lengths = lengths
            .get_mut(..len_syms + dist_syms)
            .ok_or(Error::InvalidBlock)?;
        let mut index = 0;
        while index < lengths.len() {
            let sym = self.decode(&code)?;
            let (len, repeat) = match sym {
                0..=15 => (sym as u8, 1),
                16 => {
                    let prev = *lengths
                        .get(index.wrapping_sub(1))
                        .ok_or(Error::InvalidCode)?;
                    (prev, 3 + self.bits(2)?)
                }
                17 => (0, 3 + self.bits(3)?),
                _ => (0, 11 + self.bits(7)?),
            };
            let end = index + repeat as usize;
            lengths
                .get_mut(index..end)
                .ok_or(Error::InvalidCode)?
                .fill(len);
            index = end;
        }

        // A block without an end-of-block code could never finish.
        if lengths.get(256) == Some(&0) {
            return Err(Error::InvalidCode);
        }
        match self.compressed(lengths, len_syms)? {
            true => Ok(()),
            false => Err(Error::InvalidCode),
        }
    }

    /// Decodes a compressed block given the code length of each literal/length
    /// symbol, directly followed by those of the distance symbols. Returns
    /// whether both codes were ones deflate permits.
    #[cfg(any(feature = "fixed", feature = "dynamic"))]
    fn compressed(&mut self, lengths: &[u8], len_syms: usize) -> Result<bool, Error> {
        let (len_lengths, dist_lengths) = lengths
            .split_at_checked(len_syms)
            .ok_or(Error::InvalidBlock)?;
        let mut symbol = [0; MAX_SYMS];
        let (len_symbol, dist_symbol) = symbol.split_at_mut(MAX_LEN_SYMS);
        let mut len_code = Huffman {
            count: [0; MAX_BITS + 1],
            symbol: len_symbol,
        };
        let mut dist_code = Huffman {
            count: [0; MAX_BITS + 1],
            symbol: dist_symbol,
        };
        let len_left = len_code.build(len_lengths);
        let dist_left = dist_code.build(dist_lengths);
        #[cfg(feature = "dynamic")]
        if !len_code.is_valid(len_left, len_lengths.len())
            || !dist_code.is_valid(dist_left, dist_lengths.len())
        {
            return Ok(false);
        }
        #[cfg(not(feature = "dynamic"))]
        let _ = (len_left, dist_left);

        loop {
            let sym = self.decode(&len_code)? as u32;
            if sym < 256 {
                self.out.put(sym as u8, self.check)?;
                continue;
            }
            if sym == 256 {
                return Ok(true);
            }

            // Lengths 3..=258 from symbols 257..=285: four symbols for each
            // count of extra bits, except at both ends.
            let sym = sym - 257;
            let len = match sym {
                0..=3 => sym + 3,
                4..=27 => {
                    let extra = (sym - 4) >> 2;
                    ((4 + (sym & 3)) << extra) + 3 + self.bits(extra)?
                }
                28 => 258,
                _ => return Err(Error::InvalidCode),
            };

            // Distances 1..=32768 from symbols 0..=29, two symbols for each
            // count of extra bits.
            let sym = self.decode(&dist_code)? as u32;
            let dist = match sym {
                0..=1 => sym + 1,
                2..=29 => {
                    let extra = (sym - 2) >> 1;
                    ((2 + (sym & 1)) << extra) + 1 + self.bits(extra)?
                }
                _ => return Err(Error::InvalidCode),
            };

            self.out.copy(dist as usize, len as usize, self.check)?;
        }
    }

    #[cfg(any(feature = "fixed", feature = "dynamic"))]
    fn decode(&mut self, code: &Huffman) -> Result<u16, Error> {
        let mut bits = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for &count in &code.count[1..] {
            let count = count as i32;
            bits |= self.bits(1)? as i32;
            if bits - count < first {
                return code
                    .symbol
                    .get((index + bits - first) as usize)
                    .copied()
                    .ok_or(Error::InvalidCode);
            }
            index += count;
            first = (first + count) << 1;
            bits <<= 1;
        }
        Err(Error::InvalidCode)
    }
}
