//! `gzip -dc` lookalike: decompresses standard input to standard output,
//! streaming both ways. Accepts gzip and zlib streams.
//!
//!     cargo run --release --example gunzip < file.gz > file
//!     cargo run --release --example gunzip -- --len < file.gz

use std::io::{self, Read, Write};
use std::process::ExitCode;

use minizlib::{Error, NO_LIMIT, Reader, Stream, decompress, decompress_len};

fn main() -> ExitCode {
    let len_only = std::env::args().nth(1).as_deref() == Some("--len");
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    // The library only knows about `Error::Io`; keep the cause on the side.
    let mut cause = None;
    let mut scratch = [0; 4096];
    let input = Reader::new(&mut scratch, |buf| {
        stdin.read(buf).map_err(|error| {
            cause = Some(error);
            Error::Io
        })
    });

    let result = if len_only {
        decompress_len(input, NO_LIMIT).map(|len| println!("{len}"))
    } else {
        let mut window = vec![0; 32768];
        let output = Stream::new(&mut window, NO_LIMIT, |data| {
            stdout.write_all(data).map_err(|_| Error::Io)
        });
        decompress(input, output).map(drop)
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            match cause {
                Some(cause) => eprintln!("gunzip: {error}: {cause}"),
                None => eprintln!("gunzip: {error}"),
            }
            ExitCode::FAILURE
        }
    }
}
