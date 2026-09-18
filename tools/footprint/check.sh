#!/bin/sh
# Builds the footprint binary for a bare-metal target in a range of
# configurations, prints the code size of each, and fails if any of them links
# panic machinery or needs static RAM.
#
# Needs the thumbv7em-none-eabi target and the llvm-tools component:
#     rustup target add thumbv7em-none-eabi
#     rustup component add llvm-tools
set -eu
cd "$(dirname "$0")"

TARGET=thumbv7em-none-eabi
BIN=target/$TARGET/release/footprint
TOOLS=$(dirname "$(find "$(rustc --print sysroot)" -name 'llvm-size*' | head -n 1)")
export RUSTFLAGS="-C link-arg=--gc-sections -C link-arg=--entry=entry"

status=0
check() {
    label=$1 entry=$2 features=$3
    list=$entry
    for feature in $features; do
        list="$list,minizlib/$feature"
    done
    cargo build --quiet --release --target $TARGET --features "$list"

    # Berkeley format: text, data, bss on the second line.
    set -- $("$TOOLS/llvm-size" $BIN | tail -n 1)
    panics=$("$TOOLS/llvm-nm" --demangle $BIN | grep -ci panic || true)
    printf '| %-46s | %5d | %4d | %6d |\n' "$label" "$1" "$(($2 + $3))" "$panics"
    if [ "$panics" -ne 0 ] || [ "$(($2 + $3))" -ne 0 ]; then
        status=1
    fi
}

echo '| configuration                                  | code  | RAM  | panics |'
echo '|------------------------------------------------|------:|-----:|-------:|'
check 'gunzip: buffer out, default features' buffer 'decompress gzip checksum concat stored fixed dynamic'
check '- with crc-table' buffer 'decompress gzip checksum crc-table concat stored fixed dynamic'
check '- without concat' buffer 'decompress gzip checksum stored fixed dynamic'
check '- without concat and checksum' buffer 'decompress gzip stored fixed dynamic'
check 'gunzip: buffer out, dynamic blocks only' buffer 'decompress gzip dynamic'
check 'gunzip: buffer out, fixed blocks only' buffer 'decompress gzip fixed'
check 'gunzip: buffer out, stored blocks only' buffer 'decompress gzip stored'
check 'gunzip: stream in / stream out' stream 'decompress gzip checksum concat stored fixed dynamic'
check 'gunzip: length only' len 'decompress gzip checksum concat stored fixed dynamic'
check 'gzip: buffer in, buffer out' compress 'compress gzip'
check 'gzip: stream in, stream out' compress-stream 'compress gzip'
exit $status
