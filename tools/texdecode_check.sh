#!/bin/sh
# Decodes a batch of textures (engine/target/release/assets ISO texbatch IN NATIVE) with the
# recompiled game's own GX texture decoder (../bustin-out-recomp/gcn-recomp/runtime/gx/
# texture.cpp, its decoding functions taken from the file as it is at check time; nothing of it
# is kept here) and compares the result with the native decoding, byte for byte.
# Usage: tools/texdecode_check.sh IN NATIVE WORKDIR
set -e
IN=$1; NATIVE=$2; W=$3
SRC=$(dirname "$0")/../../bustin-out-recomp/gcn-recomp/runtime/gx/texture.cpp
mkdir -p "$W"
# the decoding part: from rgba() to the end of decode_level()
awk '/^static inline uint32_t rgba\(/{p=1} /^\/\/ FNV-1a over/{p=0} p' "$SRC" > "$W/decode.inc"
cat > "$W/harness.cpp" <<'CPP'
#include <cstdint>
#include <cstdio>
#include <vector>
namespace gx {
enum TexFmt : uint32_t {
    TF_I4 = 0, TF_I8 = 1, TF_IA4 = 2, TF_IA8 = 3, TF_RGB565 = 4, TF_RGB5A3 = 5, TF_RGBA8 = 6,
    TF_CI4 = 8, TF_CI8 = 9, TF_CI14X2 = 10, TF_CMPR = 14,
};
#include "decode.inc"
}
int main(int argc, char** argv) {
    FILE* in = std::fopen(argv[1], "rb");
    FILE* out = std::fopen(argv[2], "wb");
    uint32_t h[4];
    while (std::fread(h, 4, 4, in) == 4) {
        std::vector<uint8_t> src(h[3]);
        if (std::fread(src.data(), 1, h[3], in) != h[3]) return 1;
        std::vector<uint32_t> px(h[1] * h[2]);
        gx::decode_level(src.data(), h[0], h[1], h[2], nullptr, 0, px.data());
        std::fwrite(px.data(), 4, px.size(), out);
    }
    return 0;
}
CPP
c++ -O2 -std=c++17 -w -o "$W/harness" "$W/harness.cpp"
"$W/harness" "$IN" "$W/reference.bin"
if cmp -s "$NATIVE" "$W/reference.bin"; then
    echo "the same: $(wc -c < "$NATIVE") bytes"
else
    echo "DIFFERENT:"; cmp "$NATIVE" "$W/reference.bin" | head -3
fi
