//! # Deprecated: superseded by [compcol](https://crates.io/crates/compcol)
//!
//! This crate's code now lives on in compcol, as
//! [`compcol::embed::flate`](https://docs.rs/compcol/latest/compcol/embed/flate/),
//! with the same API, and this crate only re-exports it: nothing changes for
//! code that uses it, but it gets no further work. Depend on compcol instead:
//!
//! ```toml
//! compcol = { version = "0.7.2", default-features = false, features = ["embed", "gzip"] }
//! ```
//!
//! and `use compcol::embed::flate::*` where you had `use minizlib::*`.
//! compcol's `embed` mode also has the same deflate family behind its
//! uniform `Encoder` / `Decoder` traits, alongside dozens of other formats.
//!
//! The `checksum`, `crc-table`, `concat`, `stored`, `fixed` and `dynamic`
//! features no longer change anything: checksums are always verified,
//! concatenated gzip members and all three block types always decoded.
//!
//! A tiny gzip / zlib / deflate compressor and decompressor: `no_std`, no
//! allocation, no `unsafe`, no panics, and as little code as possible.
//!
//! Pick a format, an input and an output, and go:
//!
//! | format                 | decompress        | length only            | compress         |
//! |------------------------|-------------------|------------------------|------------------|
//! | gzip (RFC 1952)        | [`gunzip`][g]     | [`gunzip_len`][gl]     | [`gzip`][cg]     |
//! | zlib (RFC 1950)        | [`unzlib`][z]     | [`unzlib_len`][zl]     | [`zlib`][cz]     |
//! | raw deflate (RFC 1951) | [`inflate`][i]    | [`inflate_len`][il]    | [`deflate`][cd]  |
//! | gzip or zlib, detected | [`decompress`][d] | [`decompress_len`][dl] |                  |
//!
//! [g]: fn.gunzip.html
//! [gl]: fn.gunzip_len.html
//! [z]: fn.unzlib.html
//! [zl]: fn.unzlib_len.html
//! [i]: fn.inflate.html
//! [il]: fn.inflate_len.html
//! [d]: fn.decompress.html
//! [dl]: fn.decompress_len.html
//! [cg]: fn.gzip.html
//! [cz]: fn.zlib.html
//! [cd]: fn.deflate.html
//! [c]: struct.Compressor.html
//! [bc]: struct.BufferedCompressor.html
//! [dc]: struct.Decompressor.html
//!
//! | input to decompress  | how                           |
//! |----------------------|-------------------------------|
//! | buffer in            | `&[u8]`, or `&mut &[u8]`      |
//! | stream in (callback) | [`Reader`]                    |
//! | stream in (iterator) | [`Bytes`]                     |
//! | stream in (pushed)   | [`Decompressor`][dc], a piece at a time |
//! | anything else        | implement [`Input`]           |
//!
//! | input to compress    | how                           |
//! |----------------------|-------------------------------|
//! | buffer in            | `&[u8]`                       |
//! | stream in            | [`Compressor`][c], a chunk at a time |
//! | stream in (pushed)   | [`BufferedCompressor`][bc], a piece at a time |
//!
//! | output                | how                 | memory needed               |
//! |-----------------------|---------------------|-----------------------------|
//! | buffer out            | [`Buffer`]          | the output buffer itself    |
//! | stream out (callback) | [`Stream`]          | a window, usually 32 KiB    |
//! | none, just the length | [`Counter`]         | none                        |
//!
//! On top of that, decompressing uses about 1.5 KiB of stack, or 1.1 KiB in a
//! [`Decompressor`][dc], which has to keep it between pieces. Compressing
//! uses next to none, and a table of yours to find matches with: any size
//! will do, 8 KiB is a good deal.
//!
//! The pushed inputs are for when the input is not yours to ask for, and
//! comes in pieces of whatever size, over a socket or a UART say: everything
//! else pulls its input as it needs it, and does not return until done.
//!
//! Every path is bounded: a [`Buffer`] by its size, a [`Stream`], a
//! [`Counter`] and the length functions by a mandatory maximum length, beyond
//! which they fail with [`Error::OutputFull`] instead of letting a
//! decompression bomb run. [`NO_LIMIT`] opts out.
//!
//! # Examples
//!
//! Buffer to buffer:
//!
//! ```
//! # #[cfg(all(feature = "gzip", feature = "decompress"))] {
//! use minizlib::{gunzip, Buffer};
//!
//! let gz = [
//!     0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
//!     0x57, 0xc8, 0x40, 0x27, 0x01, 0xe3, 0x51, 0x3d, 0x8d, 0x17, 0x00, 0x00, 0x00,
//! ];
//! let mut out = [0; 64];
//! let len = gunzip(&gz[..], Buffer::new(&mut out))? as usize;
//! assert_eq!(&out[..len], b"hello hello hello hello");
//! # }
//! # Ok::<(), minizlib::Error>(())
//! ```
//!
//! Stream to stream, sizing things up first:
//!
//! ```
//! # #[cfg(all(feature = "gzip", feature = "decompress"))] {
//! use minizlib::{gunzip, gunzip_len, Error, Reader, Stream};
//!
//! # let gz = [
//! #     0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
//! #     0x57, 0xc8, 0x40, 0x27, 0x01, 0xe3, 0x51, 0x3d, 0x8d, 0x17, 0x00, 0x00, 0x00,
//! # ];
//! assert_eq!(gunzip_len(&gz[..], 1 << 20)?, 23);
//!
//! let mut source = gz.chunks(5); // stands for a UART, a flash driver, a socket...
//! let mut scratch = [0; 16];
//! let input = Reader::new(&mut scratch, |buf| {
//!     let chunk = source.next().unwrap_or(&[]);
//!     buf[..chunk.len()].copy_from_slice(chunk);
//!     Ok(chunk.len())
//! });
//!
//! let mut window = [0; 32768];
//! let mut total = 0;
//! let output = Stream::new(&mut window, 1 << 20, |data| {
//!     total += data.len();
//!     Ok(())
//! });
//!
//! gunzip(input, output)?;
//! assert_eq!(total, 23);
//! # }
//! # Ok::<(), minizlib::Error>(())
//! ```
//!
//! Pushed in, as the compressed data comes:
//!
//! ```
//! # #[cfg(all(feature = "gzip", feature = "decompress"))] {
//! use minizlib::{Buffer, Decompressor, Gzip};
//!
//! # let gz = [
//! #     0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
//! #     0x57, 0xc8, 0x40, 0x27, 0x01, 0xe3, 0x51, 0x3d, 0x8d, 0x17, 0x00, 0x00, 0x00,
//! # ];
//! let mut out = [0; 64];
//! let mut decompressor = Decompressor::<_, Gzip>::new(Buffer::new(&mut out));
//! for piece in gz.chunks(3) { // whatever the socket hands over
//!     decompressor.write(piece)?;
//! }
//! let len = decompressor.finish()? as usize;
//! assert_eq!(&out[..len], b"hello hello hello hello");
//! # }
//! # Ok::<(), minizlib::Error>(())
//! ```
//!
//! Compressing, in one go, then a chunk at a time, then pushed in:
//!
//! ```
//! # #[cfg(all(feature = "gzip", feature = "compress", feature = "decompress"))] {
//! use minizlib::{gunzip, gzip, Buffer, BufferedCompressor, Compressor, Gzip};
//!
//! let data = b"hello hello hello hello";
//! let mut table = [0; 256];
//! let mut gz = [0; 64];
//! let whole = gzip(data, &mut table, Buffer::new(&mut gz))? as usize;
//! assert!(whole < data.len() + 18);
//!
//! let mut out = [0; 64];
//! let mut compressor = Compressor::<_, Gzip>::new(Buffer::new(&mut out), &mut table);
//! for chunk in data.chunks(10) {
//!     compressor.write(chunk)?;
//! }
//! let len = compressor.finish()? as usize;
//!
//! let mut back = [0; 64];
//! let back_len = gunzip(&out[..len], Buffer::new(&mut back))? as usize;
//! assert_eq!(&back[..back_len], data);
//!
//! // Pieces of any size compress as well as chunks the size of the buffer:
//! // here, as the whole.
//! let mut buffer = [0; 32];
//! let mut compressor =
//!     BufferedCompressor::<_, Gzip>::new(Buffer::new(&mut out), &mut table, &mut buffer);
//! for piece in data.chunks(1) {
//!     compressor.write(piece)?;
//! }
//! assert_eq!(compressor.finish()? as usize, whole);
//! # }
//! # Ok::<(), minizlib::Error>(())
//! ```
//!
//! # Features
//!
//! - `decompress`, `compress`: the two halves of the crate.
//! - `gzip`, `zlib`: the containers. Raw deflate is always available.
//! - `checksum`, `crc-table`, `concat`, `stored`, `fixed`, `dynamic`: accepted
//!   for compatibility, without effect.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[doc(inline)]
pub use compcol::embed::flate::{
    Buffer, Bytes, Checksum, Container, Counter, Error, Format, Input, NO_LIMIT, Output, Raw,
    Reader, Stream,
};

#[cfg(all(feature = "decompress", feature = "gzip", feature = "zlib"))]
#[doc(inline)]
pub use compcol::embed::flate::Detect;
#[cfg(feature = "gzip")]
#[doc(inline)]
pub use compcol::embed::flate::Gzip;
#[cfg(feature = "zlib")]
#[doc(inline)]
pub use compcol::embed::flate::Zlib;

#[cfg(feature = "decompress")]
#[doc(inline)]
pub use compcol::embed::flate::{Decompressor, inflate, inflate_len};
#[cfg(all(feature = "decompress", feature = "gzip", feature = "zlib"))]
#[doc(inline)]
pub use compcol::embed::flate::{decompress, decompress_len};
#[cfg(all(feature = "decompress", feature = "gzip"))]
#[doc(inline)]
pub use compcol::embed::flate::{gunzip, gunzip_len, gzip_size_hint};
#[cfg(all(feature = "decompress", feature = "zlib"))]
#[doc(inline)]
pub use compcol::embed::flate::{unzlib, unzlib_len};

#[cfg(all(feature = "compress", feature = "gzip"))]
#[doc(inline)]
pub use compcol::embed::flate::gzip;
#[cfg(all(feature = "compress", feature = "zlib"))]
#[doc(inline)]
pub use compcol::embed::flate::zlib;
#[cfg(feature = "compress")]
#[doc(inline)]
pub use compcol::embed::flate::{BufferedCompressor, Compressor, deflate};
