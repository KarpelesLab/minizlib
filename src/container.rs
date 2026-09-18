//! The gzip (RFC 1952) and zlib (RFC 1950) containers.

#[cfg(all(feature = "zlib", feature = "checksum"))]
use crate::checksum::Adler32;
#[cfg(all(feature = "gzip", feature = "checksum"))]
use crate::checksum::Crc32;
use crate::deflate::inflate;
use crate::io::le;
use crate::{Error, Input, Output};

/// Decodes a gzip stream whose first byte, `first`, was already read.
#[cfg(feature = "gzip")]
pub(crate) fn gzip<I: Input, O: Output>(
    first: u8,
    input: &mut I,
    out: &mut O,
) -> Result<(), Error> {
    if first != 0x1f {
        return Err(Error::InvalidHeader);
    }
    member(input, out)?;
    // Another member may follow. Anything else is ignored, as gzip does with
    // trailing padding.
    #[cfg(feature = "concat")]
    loop {
        match input.byte() {
            Ok(0x1f) => member(input, out)?,
            Ok(_) | Err(Error::UnexpectedEof) => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Decodes one gzip member, past the first byte of its header.
#[cfg(feature = "gzip")]
fn member<I: Input, O: Output>(input: &mut I, out: &mut O) -> Result<(), Error> {
    const FHCRC: u8 = 1 << 1;
    const FEXTRA: u8 = 1 << 2;
    const FNAME: u8 = 1 << 3;
    const FCOMMENT: u8 = 1 << 4;
    const RESERVED: u8 = 0xe0;

    if input.byte()? != 0x8b {
        return Err(Error::InvalidHeader);
    }
    if input.byte()? != 8 {
        return Err(Error::Unsupported);
    }
    let flags = input.byte()?;
    if flags & RESERVED != 0 {
        return Err(Error::InvalidHeader);
    }
    // MTIME, XFL and OS, then the extra field.
    skip(input, 6)?;
    if flags & FEXTRA != 0 {
        let len = le(input, 2)?;
        skip(input, len)?;
    }
    for field in [FNAME, FCOMMENT] {
        if flags & field != 0 {
            while input.byte()? != 0 {}
        }
    }
    if flags & FHCRC != 0 {
        // The header CRC is not verified: gzip never writes one.
        skip(input, 2)?;
    }

    #[cfg(feature = "checksum")]
    let (mut check, start) = (Crc32::new(), out.written());
    #[cfg(not(feature = "checksum"))]
    let mut check = ();
    inflate(input, out, &mut check)?;
    let crc = le(input, 4)?;
    let size = le(input, 4)?;
    #[cfg(feature = "checksum")]
    if (O::VERIFY && crc != check.value()) || size != (out.written() - start) as u32 {
        return Err(Error::ChecksumMismatch);
    }
    #[cfg(not(feature = "checksum"))]
    let _ = (crc, size);
    Ok(())
}

#[cfg(feature = "gzip")]
fn skip<I: Input>(input: &mut I, bytes: u32) -> Result<(), Error> {
    for _ in 0..bytes {
        input.byte()?;
    }
    Ok(())
}

/// Decodes a zlib stream whose first byte, `first`, was already read.
#[cfg(feature = "zlib")]
pub(crate) fn zlib<I: Input, O: Output>(
    first: u8,
    input: &mut I,
    out: &mut O,
) -> Result<(), Error> {
    const FDICT: u8 = 1 << 5;

    let flags = input.byte()?;
    if !(first as u32 * 256 + flags as u32).is_multiple_of(31) {
        return Err(Error::InvalidHeader);
    }
    // Deflate with a window of at most 32 KiB, and no preset dictionary.
    if first & 0x0f != 8 || first >> 4 > 7 || flags & FDICT != 0 {
        return Err(Error::Unsupported);
    }

    #[cfg(feature = "checksum")]
    let mut check = Adler32::new();
    #[cfg(not(feature = "checksum"))]
    let mut check = ();
    inflate(input, out, &mut check)?;
    let adler = le(input, 4)?.swap_bytes();
    #[cfg(feature = "checksum")]
    if O::VERIFY && adler != check.value() {
        return Err(Error::ChecksumMismatch);
    }
    #[cfg(not(feature = "checksum"))]
    let _ = adler;
    Ok(())
}
