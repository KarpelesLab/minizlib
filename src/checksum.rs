//! Running checksums over the decompressed data.

/// A running checksum fed by an [`Output`](crate::Output).
///
/// Outputs hand every decompressed byte to `update` exactly once, in order,
/// in whatever chunking is convenient for them.
pub trait Checksum {
    /// Feeds `data` into the checksum.
    fn update(&mut self, data: &[u8]);
}

/// No checksum: raw deflate, or the `checksum` feature is disabled.
impl Checksum for () {
    #[inline(always)]
    fn update(&mut self, _: &[u8]) {}
}

#[cfg(feature = "gzip")]
pub(crate) use crc32::Crc32;

#[cfg(feature = "gzip")]
mod crc32 {
    const POLY: u32 = 0xedb8_8320;

    const fn table<const N: usize>() -> [u32; N] {
        let mut table = [0; N];
        let mut i = 0;
        while i < N {
            let mut c = i as u32;
            let mut bit = N;
            while bit > 1 {
                c = if c & 1 != 0 { POLY ^ (c >> 1) } else { c >> 1 };
                bit >>= 1;
            }
            table[i] = c;
            i += 1;
        }
        table
    }

    #[cfg(feature = "crc-table")]
    static TABLE: [u32; 256] = table();
    #[cfg(not(feature = "crc-table"))]
    static TABLE: [u32; 16] = table();

    /// The CRC-32 so far, stored complemented, so that the initial state is
    /// zero: a codec built in a `static` then lands in `.bss` rather than
    /// in `.data`, and costs no flash.
    pub(crate) struct Crc32(u32);

    impl Crc32 {
        pub(crate) const fn new() -> Self {
            Crc32(0)
        }

        pub(crate) fn value(&self) -> u32 {
            self.0
        }
    }

    impl super::Checksum for Crc32 {
        fn update(&mut self, data: &[u8]) {
            let mut c = !self.0;
            for &b in data {
                c ^= b as u32;
                #[cfg(feature = "crc-table")]
                {
                    c = TABLE[(c & 0xff) as usize] ^ (c >> 8);
                }
                #[cfg(not(feature = "crc-table"))]
                {
                    c = TABLE[(c & 0xf) as usize] ^ (c >> 4);
                    c = TABLE[(c & 0xf) as usize] ^ (c >> 4);
                }
            }
            self.0 = !c;
        }
    }
}

/// The Adler-32 so far, `a` stored one less than it is (wrapping), so that
/// the initial state is zero: a codec built in a `static` then lands in
/// `.bss` rather than in `.data`, and costs no flash.
#[cfg(feature = "zlib")]
pub(crate) struct Adler32 {
    a: u32,
    b: u32,
}

#[cfg(feature = "zlib")]
impl Adler32 {
    pub(crate) const fn new() -> Self {
        Adler32 { a: 0, b: 0 }
    }

    pub(crate) fn value(&self) -> u32 {
        self.b << 16 | self.a.wrapping_add(1)
    }
}

#[cfg(feature = "zlib")]
impl Checksum for Adler32 {
    fn update(&mut self, data: &[u8]) {
        let mut a = self.a.wrapping_add(1);
        let mut b = self.b;
        // 5552 is the most bytes that can be summed before `b` overflows.
        for chunk in data.chunks(5552) {
            for &x in chunk {
                a += x as u32;
                b += a;
            }
            a %= 65521;
            b %= 65521;
        }
        self.a = a.wrapping_sub(1);
        self.b = b;
    }
}
