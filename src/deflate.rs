//! The deflate encoder.
//!
//! Greedy LZ77 over a single-probe hash table, coded with the fixed Huffman
//! codes: no code to build, nothing to buffer, a single pass. The table is the
//! caller's, and holds the low sixteen bits of the last position at which each
//! hash was seen. That is enough to tell a distance within deflate's 32 KiB
//! reach; what it cannot tell, such as a stale or never written entry, is
//! caught by comparing the data, which has to be done anyway.

#[cfg(any(feature = "gzip", feature = "zlib"))]
use crate::Checksum;
#[cfg(feature = "zlib")]
use crate::checksum::Adler32;
#[cfg(feature = "gzip")]
use crate::checksum::Crc32;
use crate::{Error, Output};

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MAX_DIST: usize = 32768;
/// A match of the minimum length costs more than literals from this far.
const TOO_FAR: usize = 4096;

const END_OF_BLOCK: u32 = 256;

mod sealed {
    pub trait Sealed {}
}

/// A container for compressed data: [`Gzip`](struct.Gzip.html),
/// [`Zlib`](struct.Zlib.html) or [`Raw`].
pub trait Format: sealed::Sealed {
    #[doc(hidden)]
    const HEADER: &'static [u8];
    /// How many of the `trailer` words to write.
    #[doc(hidden)]
    const TRAILER: usize;
    #[doc(hidden)]
    fn new() -> Self;
    #[doc(hidden)]
    fn update(&mut self, data: &[u8]);
    /// The trailer, as little-endian words, given the length of the data
    /// modulo 2<sup>32</sup>.
    #[doc(hidden)]
    fn trailer(&self, size: u32) -> [u32; 2];
}

/// The gzip format (RFC 1952), as produced by `gzip`.
#[cfg(feature = "gzip")]
pub struct Gzip(Crc32);

#[cfg(feature = "gzip")]
impl sealed::Sealed for Gzip {}

#[cfg(feature = "gzip")]
impl Format for Gzip {
    // Deflate, no flags, no modification time, no extra flags, unknown OS.
    const HEADER: &'static [u8] = &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    const TRAILER: usize = 2;

    fn new() -> Self {
        Gzip(Crc32::new())
    }

    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn trailer(&self, size: u32) -> [u32; 2] {
        [self.0.value(), size]
    }
}

/// The zlib format (RFC 1950).
#[cfg(feature = "zlib")]
pub struct Zlib(Adler32);

#[cfg(feature = "zlib")]
impl sealed::Sealed for Zlib {}

#[cfg(feature = "zlib")]
impl Format for Zlib {
    // Deflate with a 32 KiB window, fastest algorithm, no dictionary.
    const HEADER: &'static [u8] = &[0x78, 0x01];
    const TRAILER: usize = 1;

    fn new() -> Self {
        Zlib(Adler32::new())
    }

    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn trailer(&self, _: u32) -> [u32; 2] {
        // Big-endian.
        [self.0.value().swap_bytes(), 0]
    }
}

/// A raw deflate stream (RFC 1951): no header, no checksum.
pub struct Raw;

impl sealed::Sealed for Raw {}

impl Format for Raw {
    const HEADER: &'static [u8] = &[];
    const TRAILER: usize = 0;

    fn new() -> Self {
        Raw
    }

    fn update(&mut self, _: &[u8]) {}

    fn trailer(&self, _: u32) -> [u32; 2] {
        [0; 2]
    }
}

/// The base two logarithm of `value`, rounded down. Unlike `ilog2`, it has no
/// panic to link for a zero it is never given.
fn log2(value: u32) -> u32 {
    31 - value.leading_zeros()
}

/// Stream in: compresses data as it comes, a chunk at a time, in format `F`.
/// The one-shot functions, such as [`gzip`](fn.gzip.html), are this with a
/// single chunk.
///
/// The compressed data goes to any [`Output`]: a [`Buffer`](crate::Buffer), a
/// [`Stream`](crate::Stream), whose window then is a mere buffer, of any size,
/// or a [`Counter`](crate::Counter) to only find out how long it would be.
///
/// `table` is the memory matches get found with. Any number of entries will
/// do, of which the largest power of two, up to 65536, gets used; it need not
/// be cleared. The more the better, to a point: 4096 entries (8 KiB) is a good
/// deal, and an empty table still gets the Huffman coding done.
///
/// Matches are only looked for within a chunk, so larger chunks compress
/// better, until they reach a few times deflate's 32 KiB reach. Each chunk
/// also costs ten bits.
pub struct Compressor<'t, O, F> {
    out: O,
    format: F,
    /// What is left to write of the header.
    header: &'static [u8],
    table: &'t mut [u16],
    mask: usize,
    start: u64,
    size: u32,
    bit_buf: u32,
    bit_cnt: u32,
}

impl<'t, O: Output, F: Format> Compressor<'t, O, F> {
    /// Starts a compressed stream.
    ///
    /// Nothing is written yet, and the compressor is built in place: handing
    /// it over inside a `Result` would have it copied, and drag `memcpy` in.
    #[inline(always)]
    pub fn new(output: O, table: &'t mut [u16]) -> Self {
        let mask = match table.len().checked_ilog2() {
            Some(bits) => (1 << bits.min(16)) - 1,
            None => 0,
        };
        Compressor {
            start: output.written(),
            out: output,
            format: F::new(),
            header: F::HEADER,
            table,
            mask,
            size: 0,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    /// Compresses a chunk of data, as one deflate block.
    pub fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }
        self.format.update(data);
        self.size = self.size.wrapping_add(data.len() as u32);

        // Not the final block, fixed Huffman codes.
        self.block(0b010)?;
        let mut pos = 0;
        while let Some(&byte) = data.get(pos) {
            let (len, dist) = self.find(data, pos);
            if len < MIN_MATCH {
                self.symbol(byte as u32)?;
                pos += 1;
                continue;
            }
            pos += len;

            // The inverse of the decoder's arithmetic. Length symbols come by
            // four for each count of extra bits, distance ones by two.
            let len = (len - MIN_MATCH) as u32;
            let (sym, extra) = match len {
                0..=7 => (len, 0),
                255 => (28, 0),
                _ => {
                    let extra = log2(len) - 2;
                    (4 * extra + 4 + ((len >> extra) & 3), extra)
                }
            };
            self.symbol(257 + sym)?;
            self.bits(len & ((1 << extra) - 1), extra)?;

            let dist = (dist - 1) as u32;
            let (sym, extra) = match dist {
                0..=3 => (dist, 0),
                _ => {
                    let extra = log2(dist) - 1;
                    (2 * extra + 2 + ((dist >> extra) & 1), extra)
                }
            };
            // Distance codes are five bits long, the most significant first.
            self.bits(sym.reverse_bits() >> 27, 5)?;
            self.bits(dist & ((1 << extra) - 1), extra)?;
        }
        self.symbol(END_OF_BLOCK)
    }

    /// Ends the stream, writing its trailer and flushing the output. Returns
    /// the length of the compressed stream.
    ///
    /// The compressor is then ready for another stream, to the same output.
    /// This takes no `self`: that would be a copy, and drag `memcpy` in.
    pub fn finish(&mut self) -> Result<u64, Error> {
        // A final, empty block, then padding to a byte boundary.
        self.block(0b011)?;
        self.symbol(END_OF_BLOCK)?;
        self.bits(0, 7)?;
        self.bit_cnt = 0;

        let trailer = self.format.trailer(self.size);
        for &word in trailer.iter().take(F::TRAILER) {
            self.bits(word & 0xffff, 16)?;
            self.bits(word >> 16, 16)?;
        }
        self.out.flush(&mut ())?;

        let end = self.out.written();
        let len = end - self.start;
        self.start = end;
        self.format = F::new();
        self.header = F::HEADER;
        self.size = 0;
        Ok(len)
    }

    /// Starts a block, after the header if this is the first.
    fn block(&mut self, kind: u32) -> Result<(), Error> {
        for &byte in core::mem::take(&mut self.header) {
            self.bits(byte as u32, 8)?;
        }
        self.bits(kind, 3)
    }

    /// Looks for a match for the data at `pos`. Returns its length, zero if
    /// there is none, and distance.
    fn find(&mut self, data: &[u8], pos: usize) -> (usize, usize) {
        let ahead = data.get(pos..).unwrap_or(&[]);
        let Some(&[a, b, c]) = ahead.first_chunk() else {
            return (0, 0);
        };
        let hash = u32::from_le_bytes([a, b, c, 0]).wrapping_mul(0x9e37_79b1) >> 16;
        let Some(entry) = self.table.get_mut(hash as usize & self.mask) else {
            return (0, 0);
        };
        let dist = (pos as u16).wrapping_sub(*entry) as usize;
        *entry = pos as u16;

        // A distance of zero is 65536 bytes back, or an entry never written.
        let Some(behind) = pos.checked_sub(dist).and_then(|from| data.get(from..)) else {
            return (0, 0);
        };
        if dist == 0 || dist > MAX_DIST {
            return (0, 0);
        }
        let len = behind
            .iter()
            .zip(ahead)
            .take(MAX_MATCH)
            .take_while(|(a, b)| a == b)
            .count();
        if len == MIN_MATCH && dist > TOO_FAR {
            return (0, 0);
        }
        (len, dist)
    }

    /// Writes a literal/length symbol with its fixed Huffman code.
    fn symbol(&mut self, sym: u32) -> Result<(), Error> {
        let (code, len) = match sym {
            0..=143 => (0x30 + sym, 8),
            144..=255 => (sym - 144 + 0x190, 9),
            256..=279 => (sym - 256, 7),
            _ => (sym - 280 + 0xc0, 8),
        };
        // Huffman codes go the most significant bit first.
        self.bits(code.reverse_bits() >> (32 - len), len)
    }

    /// Writes the `count` low bits of `value`, at most 16, the least
    /// significant first. The others must be zeros.
    fn bits(&mut self, value: u32, count: u32) -> Result<(), Error> {
        self.bit_buf |= value << self.bit_cnt;
        self.bit_cnt += count;
        while self.bit_cnt >= 8 {
            self.out.put(self.bit_buf as u8, &mut ())?;
            self.bit_buf >>= 8;
            self.bit_cnt -= 8;
        }
        Ok(())
    }
}
