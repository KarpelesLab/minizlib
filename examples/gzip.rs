//! `gzip -c` lookalike: compresses standard input to standard output, a chunk
//! at a time.
//!
//!     cargo run --release --example gzip < file > file.gz
//!     cargo run --release --example gzip -- 12 65536 < file > file.gz
//!
//! The arguments are the size of the match table, as a power of two (default
//! 12, or 8 KiB) and that of the chunks (default 256 KiB).

use std::io::{self, Read, Write};
use std::process::ExitCode;

use minizlib::{Compressor, Error, Gzip, NO_LIMIT, Stream};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).map(|arg| arg.parse::<usize>());
    let (Ok(table_bits), Ok(chunk_len)) = (
        args.next().unwrap_or(Ok(12)),
        args.next().unwrap_or(Ok(256 << 10)),
    ) else {
        eprintln!("usage: gzip [table bits] [chunk length]");
        return ExitCode::FAILURE;
    };

    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut table = vec![0; 1 << table_bits];
    let mut chunk = vec![0; chunk_len.max(1)];
    let mut buffer = [0; 4096];
    let output = Stream::new(&mut buffer, NO_LIMIT, |data| {
        stdout.write_all(data).map_err(|_| Error::Io)
    });

    let mut compressor = Compressor::<_, Gzip>::new(output, &mut table);
    let result = loop {
        // Fill the chunk: matches are only found within one.
        let mut len = 0;
        let mut failed = false;
        while len < chunk.len() && !failed {
            match stdin.read(&mut chunk[len..]) {
                Ok(0) => break,
                Ok(n) => len += n,
                Err(_) => failed = true,
            }
        }
        if failed {
            break Err(Error::Io);
        }
        if let Err(error) = compressor.write(&chunk[..len]) {
            break Err(error);
        }
        if len < chunk.len() {
            break compressor.finish();
        }
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gzip: {error}");
            ExitCode::FAILURE
        }
    }
}
