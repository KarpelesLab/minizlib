# minigunzip

A tiny gzip / zlib / deflate decompressor for Rust: `no_std`, no allocation,
no `unsafe`, no dependencies, no panics, and as little code as possible.

Full gzip support links to **under 3 KB** of Thumb-2 code, and features let
you strip that down further. Buffer in or stream in, buffer out or stream out,
and go.

```rust
use minigunzip::{gunzip, Buffer};

let mut out = [0; 4096];
let len = gunzip(gz_bytes, Buffer::new(&mut out))? as usize;
let data = &out[..len];
```

## Pick a format, an input and an output

| format                 | function     | length only      |
|------------------------|--------------|------------------|
| gzip (RFC 1952)        | `gunzip`     | `gunzip_len`     |
| zlib (RFC 1950)        | `unzlib`     | `unzlib_len`     |
| raw deflate (RFC 1951) | `inflate`    | `inflate_len`    |
| gzip or zlib, detected | `decompress` | `decompress_len` |

| input                | how                      |
|----------------------|--------------------------|
| buffer in            | `&[u8]`, or `&mut &[u8]` to get the unconsumed rest back |
| stream in (callback) | `Reader::new(&mut scratch, \|buf\| ...)`, scratch of any size |
| stream in (iterator) | `Bytes(iter)`            |
| anything else        | implement `Input`, a single `fn byte()` |

| output                | how                                  | memory needed            |
|-----------------------|--------------------------------------|--------------------------|
| buffer out            | `Buffer::new(&mut out)`              | the output buffer itself |
| stream out (callback) | `Stream::new(&mut window, \|data\| ...)` | a window, usually 32 KiB |
| none, just the length | `Counter::new()`                     | none                     |

Every function takes any input with any output, and returns the number of
bytes produced. Pass `&mut input` or `&mut output` to keep using them
afterwards. Beyond the above, decoding uses about 1.5 KiB of stack.

Everything gzip can produce is supported: all three deflate block types, header
name / comment / extra fields, and concatenated members.

### Streaming

```rust
use minigunzip::{gunzip, Error, Reader, Stream};

let mut scratch = [0; 64];
let input = Reader::new(&mut scratch, |buf| uart.read(buf).map_err(|_| Error::Io));

let mut window = [0; 32768];
let output = Stream::new(&mut window, |data| flash.write(data).map_err(|_| Error::Io));

gunzip(input, output)?;
```

The window must be at least as large as the compressor's, else decoding fails
with `Error::WindowTooSmall`. 32 KiB covers everything; if you control the
compressor (`wbits` in zlib), a smaller window works with a smaller buffer.
The decoder never reads past the end of the compressed stream, apart from the
one look-ahead byte the `concat` feature needs.

### Finding the decompressed length

`gunzip_len` and friends decode the stream without storing anything. A
back-reference has a known length whatever it points at, so no window and no
output buffer are needed, and it runs two to three times faster than
decompressing. The data checksum cannot be verified this way; the structure of
the stream and the length gzip records are.

`gzip_size_hint(&gz)` just reads the length recorded in the last four bytes of
a gzip file. It is instant, but only a hint: last member only, modulo 2³², and
unverified.

## Features

Everything is on by default except `crc-table`. To strip what you don't need:

```toml
minigunzip = { version = "0.1", default-features = false, features = ["gzip", "dynamic"] }
```

| feature     | what it does |
|-------------|--------------|
| `gzip`      | the gzip container |
| `zlib`      | the zlib container |
| `checksum`  | verify CRC-32 and length (gzip) or Adler-32 (zlib); otherwise trailers are read and ignored |
| `crc-table` | 256-entry CRC-32 table (1 KiB) instead of the 16-entry one (64 B): faster, bigger |
| `concat`    | decode concatenated gzip members as one stream, like `gzip -d`; consumes one byte past each member, if any, to look for the next |
| `stored`    | deflate stored blocks |
| `fixed`     | deflate fixed Huffman blocks |
| `dynamic`   | deflate dynamic Huffman blocks |

Raw deflate is always available. At least one block type must be enabled;
streams using a disabled one fail with `Error::Unsupported`. General purpose
compressors emit all three, so only drop block types if you control the
compressor.

## Size

Code size of a function gunzipping one slice into another, everything it needs
included, on `thumbv7em-none-eabi` with `opt-level = "z"` and LTO (rustc 1.98).
No RAM is used besides the stack. `tools/footprint/check.sh` reproduces the
table, and CI runs it to keep the no-panic guarantee honest.

| configuration                                   | bytes |
|-------------------------------------------------|------:|
| gzip, default features                          |  2896 |
| … with `crc-table`                              |  3846 |
| … without `concat`                              |  2698 |
| … without `concat` and `checksum`               |  2559 |
| gzip, `dynamic` blocks only                     |  2157 |
| gzip, `fixed` blocks only                       |  1716 |
| gzip, `stored` blocks only                      |   710 |
| stream in / stream out, default features        |  3160 |
| length only, default features                   |  2781 |

About 600 of those bytes are the `memset` / `memclr` routines of
`compiler_builtins`, which most firmware links anyway. No configuration links
any panic machinery: malformed input of any kind is reported as an `Error`.

## How

Huffman codes are decoded canonically, one bit at a time, straight from the
count of codes of each length, in the manner of zlib's `puff`. It is slower
than the usual lookup tables (expect around 100 MB/s on a desktop core at
`opt-level = "z"`), but it needs next to no code, no table building, and little
stack (1.3 KiB measured on Thumb-2). Length and distance bases are computed
rather than tabulated. Checksums are fed in bulk when the output is flushed
rather than byte by byte.

What is left out: compression, preset dictionaries (`Error::Unsupported`),
exposing the gzip header fields, and verifying the optional gzip header CRC,
which gzip itself never writes.

## Testing

`cargo test` round-trips a range of data shapes through flate2 at every
compression level and every input / output combination, checks error
reporting, truncates streams at every offset, and feeds the decoder tens of
thousands of corrupted and random streams, checking all outputs agree.
`examples/gunzip.rs` is a streaming `gzip -dc` lookalike to try on real files.

## License

MIT
