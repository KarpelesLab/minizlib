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
        list="$list,minigunzip/$feature"
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

echo '| gzip configuration                             | code  | RAM  | panics |'
echo '|------------------------------------------------|------:|-----:|-------:|'
check 'buffer out, default features' buffer 'gzip checksum concat stored fixed dynamic'
check '- with crc-table' buffer 'gzip checksum crc-table concat stored fixed dynamic'
check '- without concat' buffer 'gzip checksum stored fixed dynamic'
check '- without concat and checksum' buffer 'gzip stored fixed dynamic'
check 'buffer out, dynamic blocks only' buffer 'gzip dynamic'
check 'buffer out, fixed blocks only' buffer 'gzip fixed'
check 'buffer out, stored blocks only' buffer 'gzip stored'
check 'stream in / stream out, default features' stream 'gzip checksum concat stored fixed dynamic'
check 'length only, default features' len 'gzip checksum concat stored fixed dynamic'
exit $status
