#![no_std]
#![no_main]

#[allow(unused_imports)]
use minizlib::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Buffer in, buffer out.
#[cfg(feature = "buffer")]
#[unsafe(no_mangle)]
pub extern "C" fn entry(src: *const u8, src_len: usize, dst: *mut u8, dst_len: usize) -> i64 {
    let src = unsafe { core::slice::from_raw_parts(src, src_len) };
    let dst = unsafe { core::slice::from_raw_parts_mut(dst, dst_len) };
    gunzip(src, Buffer::new(dst)).map_or(-1, |len| len as i64)
}

/// Stream in, stream out.
#[cfg(feature = "stream")]
#[unsafe(no_mangle)]
pub extern "C" fn entry(
    read: extern "C" fn(*mut u8, usize) -> usize,
    write: extern "C" fn(*const u8, usize),
    window: *mut u8,
    window_len: usize,
    max_len: u64,
) -> i64 {
    let window = unsafe { core::slice::from_raw_parts_mut(window, window_len) };
    let mut scratch = [0; 64];
    let input = Reader::new(&mut scratch, |buf| Ok(read(buf.as_mut_ptr(), buf.len())));
    let output = Stream::new(window, max_len, |data| {
        write(data.as_ptr(), data.len());
        Ok(())
    });
    gunzip(input, output).map_or(-1, |len| len as i64)
}

/// Length only.
#[cfg(feature = "len")]
#[unsafe(no_mangle)]
pub extern "C" fn entry(src: *const u8, src_len: usize, max_len: u64) -> i64 {
    let src = unsafe { core::slice::from_raw_parts(src, src_len) };
    gunzip_len(src, max_len).map_or(-1, |len| len as i64)
}

/// Compression: buffer in, buffer out.
#[cfg(feature = "compress")]
#[unsafe(no_mangle)]
pub extern "C" fn entry(
    src: *const u8,
    src_len: usize,
    dst: *mut u8,
    dst_len: usize,
    table: *mut u16,
    table_len: usize,
) -> i64 {
    let src = unsafe { core::slice::from_raw_parts(src, src_len) };
    let dst = unsafe { core::slice::from_raw_parts_mut(dst, dst_len) };
    let table = unsafe { core::slice::from_raw_parts_mut(table, table_len) };
    gzip(src, table, Buffer::new(dst)).map_or(-1, |len| len as i64)
}

/// Compression: stream in, stream out.
#[cfg(feature = "compress-stream")]
#[unsafe(no_mangle)]
pub extern "C" fn entry(
    read: extern "C" fn(*mut u8, usize) -> usize,
    write: extern "C" fn(*const u8, usize),
    table: *mut u16,
    table_len: usize,
) -> i64 {
    let table = unsafe { core::slice::from_raw_parts_mut(table, table_len) };
    let mut chunk = [0; 512];
    let mut buffer = [0; 64];
    let output = Stream::new(&mut buffer, NO_LIMIT, |data| {
        write(data.as_ptr(), data.len());
        Ok(())
    });
    let mut compressor = Compressor::<_, Gzip>::new(output, table);
    loop {
        let len = read(chunk.as_mut_ptr(), chunk.len()).min(chunk.len());
        if len == 0 {
            return compressor.finish().map_or(-1, |len| len as i64);
        }
        if compressor.write(&chunk[..len]).is_err() {
            return -1;
        }
    }
}

/// Pushed in, buffer out.
#[cfg(feature = "push")]
#[unsafe(no_mangle)]
pub extern "C" fn entry(
    read: extern "C" fn(*mut u8, usize) -> usize,
    dst: *mut u8,
    dst_len: usize,
) -> i64 {
    let dst = unsafe { core::slice::from_raw_parts_mut(dst, dst_len) };
    let mut chunk = [0; 64];
    let mut decompressor = Decompressor::<_, Gzip>::new(Buffer::new(dst));
    loop {
        let len = read(chunk.as_mut_ptr(), chunk.len()).min(chunk.len());
        if len == 0 {
            return decompressor.finish().map_or(-1, |len| len as i64);
        }
        if decompressor.write(&chunk[..len]).is_err() {
            return -1;
        }
    }
}

/// Compression: pushed in, stream out.
#[cfg(feature = "compress-push")]
#[unsafe(no_mangle)]
pub extern "C" fn entry(
    read: extern "C" fn(*mut u8, usize) -> usize,
    write: extern "C" fn(*const u8, usize),
    table: *mut u16,
    table_len: usize,
    gather: *mut u8,
    gather_len: usize,
) -> i64 {
    let table = unsafe { core::slice::from_raw_parts_mut(table, table_len) };
    let gather = unsafe { core::slice::from_raw_parts_mut(gather, gather_len) };
    let mut chunk = [0; 64];
    let mut buffer = [0; 64];
    let output = Stream::new(&mut buffer, NO_LIMIT, |data| {
        write(data.as_ptr(), data.len());
        Ok(())
    });
    let mut compressor = BufferedCompressor::<_, Gzip>::new(output, table, gather);
    loop {
        let len = read(chunk.as_mut_ptr(), chunk.len()).min(chunk.len());
        if len == 0 {
            return compressor.finish().map_or(-1, |len| len as i64);
        }
        if compressor.write(&chunk[..len]).is_err() {
            return -1;
        }
    }
}
