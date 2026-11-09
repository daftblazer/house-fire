#include "housefire/iff.h"

#include <cstring>

namespace housefire {

namespace {
const char kMagic[] = "IFF FILE 2.5:TYPE FOLLOWED BY SIZE\0 JAMIE DOORNBOS & MAXIS 1";
constexpr size_t kMagicLen = sizeof(kMagic) - 1;  // 60 bytes, the second NUL included
constexpr size_t kHeader = 64;                    // the magic and the resource map's offset
constexpr size_t kChunkHeader = 76;

uint32_t be32(const uint8_t* p) { return (uint32_t)p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]; }

bool type_char(uint8_t c) {
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '#' || c == '$' || c == ' ';
}
}  // namespace

IffFile read_iff(std::span<const uint8_t> f) {
    IffFile out;
    if (f.size() < kHeader || memcmp(f.data(), kMagic, kMagicLen) != 0) return out;
    out.ok = true;
    size_t p = kHeader;
    while (p + kChunkHeader <= f.size()) {
        const uint8_t* h = f.data() + p;
        if (!type_char(h[0]) || !type_char(h[1]) || !type_char(h[2]) || !type_char(h[3])) break;
        const uint32_t size = be32(h + 4);
        if (size < kChunkHeader || p + size > f.size()) break;
        Chunk c;
        memcpy(c.type, h, 4);
        c.id = (int16_t)(h[8] << 8 | h[9]);
        c.flags = (uint16_t)(h[10] << 8 | h[11]);
        const char* lab = (const char*)h + 12;
        c.label.assign(lab, strnlen(lab, 64));
        c.data = f.subspan(p + kChunkHeader, size - kChunkHeader);
        out.chunks.push_back(std::move(c));
        p += size;
    }
    return out;
}

std::vector<const Chunk*> IffFile::of(const char* type) const {
    std::vector<const Chunk*> v;
    for (const Chunk& c : chunks)
        if (memcmp(c.type, type, 4) == 0) v.push_back(&c);
    return v;
}

const Chunk* IffFile::find(const char* type, int16_t id) const {
    for (const Chunk& c : chunks)
        if (c.id == id && memcmp(c.type, type, 4) == 0) return &c;
    return nullptr;
}

std::span<const uint8_t> Reader::take(size_t n) {
    if (n > left()) throw FormatError("needs " + std::to_string(n) + " bytes at " + std::to_string(p_));
    auto s = d_.subspan(p_, n);
    p_ += n;
    return s;
}

uint8_t Reader::u8() { return take(1)[0]; }

uint16_t Reader::u16() {
    auto b = take(2);
    return o_ == Order::Little ? (uint16_t)(b[0] | b[1] << 8) : (uint16_t)(b[0] << 8 | b[1]);
}

uint32_t Reader::u32() {
    auto b = take(4);
    return o_ == Order::Little ? (uint32_t)b[0] | b[1] << 8 | b[2] << 16 | (uint32_t)b[3] << 24
                               : (uint32_t)b[0] << 24 | b[1] << 16 | b[2] << 8 | b[3];
}

void Reader::done() const {
    for (size_t i = p_; i < d_.size(); i++)
        if (d_[i] != 0 && d_[i] != 0xA3 && d_[i] != 0xCD)
            throw FormatError(std::to_string(d_.size() - p_) + " bytes left over");
}

}  // namespace housefire
