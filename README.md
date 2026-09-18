# minizlib

A tiny gzip / zlib / deflate compressor and decompressor for Rust: `no_std`, no
allocation, no `unsafe`, no dependencies, no panics, and as little code as
possible.

Full gzip decompression links to **under 2.5 KB** of Thumb-2 code, compression
to **about 1 KB**, and features let you strip that down further. Buffer in or
stream in, buffer out or stream out, and go.

```rust
use minizlib::{gunzip, gzip, Buffer};

let mut out = [0; 4096];
let len = gunzip(gz_bytes, Buffer::new(&mut out))? as usize;
let data = &out[..len];

let mut table = [0; 4096]; // to find matches with
let mut gz = [0; 4096];
let len = gzip(data, &mut table, Buffer::new(&mut gz))? as usize;
```

## Pick a format, an input and an output

| format                 | decompress   | length only      | compress  |
|------------------------|--------------|------------------|-----------|
| gzip (RFC 1952)        | `gunzip`     | `gunzip_len`     | `gzip`    |
| zlib (RFC 1950)        | `unzlib`     | `unzlib_len`     | `zlib`    |
| raw deflate (RFC 1951) | `inflate`    | `inflate_len`    | `deflate` |
| gzip or zlib, detected | `decompress` | `decompress_len` |           |

| input to decompress  | how                      |
|----------------------|--------------------------|
| buffer in            | `&[u8]`, or `&mut &[u8]` to get the unconsumed rest back |
| stream in (callback) | `Reader::new(&mut scratch, \|buf\| ...)`, scratch of any size |
| stream in (iterator) | `Bytes(iter)`            |
| stream in (pushed)   | `Decompressor`, a piece at a time, as it comes |
| anything else        | implement `Input`, a single `fn byte()` |

| input to compress    | how                      |
|----------------------|--------------------------|
| buffer in            | `&[u8]`                  |
| stream in            | `Compressor`, a chunk at a time |
| stream in (pushed)   | `BufferedCompressor`, a piece at a time, as it comes |

| output                | how                                  | memory needed            |
|-----------------------|--------------------------------------|--------------------------|
| buffer out            | `Buffer::new(&mut out)`              | the output buffer itself |
| stream out (callback) | `Stream::new(&mut window, max_len, \|data\| ...)` | a window, usually 32 KiB |
| none, just the length | `Counter::new(max_len)`              | none                     |

Every function takes any input with any output, and returns the number of
bytes produced. Pass `&mut input` or `&mut output` to keep using them
afterwards. Beyond the above, decompressing uses about 1.5 KiB of stack, or
1.1 KiB inside a `Decompressor`, and compressing next to none.

Every path is bounded, so a decompression bomb cannot run away: a `Buffer` by
its size, a `Stream`, a `Counter` and the `*_len` functions by a mandatory
`max_len`, past which they fail with `Error::OutputFull`. `NO_LIMIT` is the
explicit way out, for when whatever comes really is welcome.

Everything gzip can produce is supported: all three deflate block types, header
name / comment / extra fields, and concatenated members.

### Streaming

```rust
use minizlib::{gunzip, Error, Reader, Stream};

let mut scratch = [0; 64];
let input = Reader::new(&mut scratch, |buf| uart.read(buf).map_err(|_| Error::Io));

let mut window = [0; 32768];
let output = Stream::new(&mut window, PARTITION_SIZE, |data| flash.write(data).map_err(|_| Error::Io));

gunzip(input, output)?;
```

The window must be at least as large as the compressor's, else decoding fails
with `Error::WindowTooSmall`. 32 KiB covers everything; if you control the
compressor (`wbits` in zlib), a smaller window works with a smaller buffer.
The decoder never reads past the end of the compressed stream, apart from the
one look-ahead byte the `concat` feature needs.

### Pushing

The above pull their input as they need it, and return once done. When the
input is not yours to ask for, and comes in pieces of whatever size a socket,
a UART or a protocol hands over, push it instead:

```rust
use minizlib::{Decompressor, Detect, Error, Stream, NO_LIMIT};

let mut window = [0; 32768];
let output = Stream::new(&mut window, NO_LIMIT, |data| flash.write(data).map_err(|_| Error::Io));

let mut decompressor = Decompressor::<_, Detect>::new(output); // gzip or zlib, whichever comes
while let Some(piece) = socket.next_piece() {
    decompressor.write(piece)?; // a byte or a megabyte, either way
}
let len = decompressor.finish()?;
```

`Gzip`, `Zlib`, `Raw` and `Detect` pick the container. Every `write` decodes
as far as the input goes and delivers the result, without waiting for a window
to fill up, and returns how much it consumed: all of it, unless the stream
ended first. `finish` fails with `Error::UnexpectedEof` if the stream did not.
The context the decoder keeps between pieces, the codes of the current block
mostly, is the 1.1 KiB the `Decompressor` itself takes. It runs at about 90 %
of the speed of the pull decoder, 70 % when fed a byte at a time.

### Compressing

```rust
use minizlib::{Compressor, Gzip, Stream, NO_LIMIT};

let mut table = [0; 4096];
let mut buffer = [0; 64];
let output = Stream::new(&mut buffer, NO_LIMIT, |data| uart.write(data).map_err(|_| Error::Io));

let mut compressor = Compressor::<_, Gzip>::new(output, &mut table);
while let Some(chunk) = sensor.next_chunk() {
    compressor.write(chunk)?;
}
let compressed_len = compressor.finish()?;
```

The compressor is greedy LZ77 over a single-probe hash table, coded with
deflate's fixed Huffman codes: one pass, nothing to buffer, no code to build.
The outputs are the decompressor's: a `Buffer`, a `Stream` (whose window is
then a mere buffer, of any size), or a `Counter` to only learn the compressed
length. `gzip`, `zlib` and `deflate` are a `Compressor` given a single chunk.

The table is yours: any number of `u16`, of which the largest power of two gets
used. It need not be cleared, as what it holds is checked against the data. An
empty one still gets the Huffman coding done. On 8.7 MB of source code:

| table                     | compressed |
|---------------------------|-----------:|
| none                      |      89 %  |
| 256 entries (512 B)       |      41 %  |
| 1024 entries (2 KiB)      |      34 %  |
| 4096 entries (8 KiB)      |      32 %  |
| 65536 entries (128 KiB)   |      31 %  |
| `gzip -1`, for comparison |      24 %  |
| `gzip -6`                 |      19 %  |

Matches are only looked for within a chunk, so larger chunks compress better
(34 % with chunks of 32 KiB, 42 % with 4 KiB, same table of 4096), and each
chunk costs ten bits. What cannot be compressed grows by an eighth at worst.

When the chunks are not yours to choose, `BufferedCompressor::new(output,
&mut table, &mut buffer)` takes pieces of any size, a byte at a time if need
be, gathers them in `buffer`, and compresses it each time it fills up: a
32 KiB buffer gets the 32 KiB chunk figure above whatever the pieces, and
what comes out does not depend on how the data was cut. A buffer's worth
pushed at once is compressed from where it is, without a copy.

The price of the small code is the ratio: fixed Huffman codes and no lazy
matching. Entering the positions that a match skips into the table would gain
4 % for 100 bytes of code and a third more time; it was left out.

### Finding the decompressed length

`gunzip_len(input, max_len)` and friends decode the stream without storing
anything. A
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
minizlib = { version = "0.1", default-features = false, features = ["decompress", "gzip", "dynamic"] }
```

| feature      | what it does |
|--------------|--------------|
| `decompress` | the decompressor |
| `compress`   | the compressor |
| `gzip`       | the gzip container |
| `zlib`       | the zlib container |
| `checksum`   | when decompressing, verify CRC-32 and length (gzip) or Adler-32 (zlib); otherwise trailers are read and ignored |
| `crc-table`  | 256-entry CRC-32 table (1 KiB) instead of the 16-entry one (64 B): faster, bigger |
| `concat`     | decompress concatenated gzip members as one stream, like `gzip -d`; consumes one byte past each member, if any, to look for the next |
| `stored`     | decompress deflate stored blocks |
| `fixed`      | decompress deflate fixed Huffman blocks |
| `dynamic`    | decompress deflate dynamic Huffman blocks |

Raw deflate is always available. To decompress, at least one block type must
be enabled; streams using a disabled one fail with `Error::Unsupported`.
General purpose compressors emit all three, so only drop block types if you
control the compressor. This crate's own only emits `fixed`.

What you do not call does not get linked, features or not: they are there to
make sure of it, and to save on build time.

## Size

Code size of a function gunzipping, or gzipping, one slice into another,
everything it needs included, on `thumbv7em-none-eabi` with `opt-level = "z"` and LTO (rustc 1.98).
No RAM is used besides the stack. `tools/footprint/check.sh` reproduces the
table, and CI runs it to keep the no-panic guarantee honest.

| configuration                                   | bytes |
|-------------------------------------------------|------:|
| gzip, default features                          |  2436 |
| … with `crc-table`                              |  3382 |
| … without `concat`                              |  2322 |
| … without `concat` and `checksum`               |  2126 |
| gzip, `dynamic` blocks only                     |  1933 |
| gzip, `fixed` blocks only                       |  1584 |
| gzip, `stored` blocks only                      |   518 |
| stream in / stream out, default features        |  2758 |
| length only, default features                   |  2294 |
| pushed in, buffer out, default features         |  3454 |
| … without `concat` and `checksum`               |  3084 |
| … `fixed` blocks only                           |  2190 |
| **compression**: gzip, buffer in, buffer out    |  1028 |
| **compression**: gzip, stream in, stream out    |  1348 |
| **compression**: gzip, pushed in, stream out    |  1538 |

About 400 of the decompressor's bytes are the `memset` / `memclr` routines of
`compiler_builtins`, which most firmware links anyway; the compressor needs
none. No configuration links
any panic machinery: malformed input of any kind is reported as an `Error`.

## How

Huffman codes are decoded canonically, one bit at a time, straight from the
count of codes of each length, in the manner of zlib's `puff`. It is slower
than the usual lookup tables (expect around 80 MB/s on a desktop core at
`opt-level = "z"`), but it needs next to no code, no table building, and little
stack (1.3 KiB measured on Thumb-2). Length and distance bases are computed
rather than tabulated. Checksums are fed in bulk when the output is flushed
rather than byte by byte.

A few things matter more than they look, on a 32-bit target. Everything,
container headers and trailers included, is read through one primitive that
returns sixteen bits at most: a `Result<u16, Error>` comes back in a register,
where a `Result<u32, Error>` goes through the stack at every call site. That
primitive cannot even fail: once the input has, it reads as zeros, the error is
kept on the side, and the few loops that zeros would not stop, or that would
output something, check for it. And nothing large is ever returned by value,
which would drag `memcpy` in.

The push decoder is the same decoder with its loops unrolled into a state
machine, each state a step of 32 bits at most over the same primitives. A step
runs on whatever input there is; if that runs out midway, the zeros it read
guarantee that nothing was output, so the step is rolled back, the few bytes
it had taken go into the bit buffer, and it is retried when more comes. That
is what lets it keep just 1.1 KiB between pieces, and not depend on how big
they are.

What is left out: dynamic Huffman codes when compressing, preset dictionaries (`Error::Unsupported`),
exposing the gzip header fields, and verifying the optional gzip header CRC,
which gzip itself never writes.

## Testing

`cargo test` round-trips a range of data shapes through flate2 at every
compression level and every input / output combination, checks error
reporting, truncates streams at every offset, and feeds the decoder tens of
thousands of corrupted and random streams, checking all outputs agree. The
push decoder gets the same streams cut into pieces of every size, a byte at a
time included, and has to agree with the pull one on every outcome and every
byte of output; the buffered compressor has to produce the same stream however
its input is cut.
The compressor's output is checked against both this crate's decompressor and
flate2's, over data stitched from copies of every length and distance, tables
of every size, cleared or not, and chunks of every size. `examples/gunzip.rs`
and `examples/gzip.rs` are streaming lookalikes of `gzip -dc` and `gzip -c` to
try on real files.

## License

MIT
