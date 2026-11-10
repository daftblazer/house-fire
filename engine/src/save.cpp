#include "housefire/save.h"

#include <cstring>

#include "housefire/iff.h"

namespace housefire {

namespace {
constexpr int kWidths8[4] = {2, 4, 6, 8};
constexpr int kWidths16[4] = {5, 8, 13, 16};
constexpr int kWidths32[4] = {6, 11, 21, 32};
constexpr size_t kBlock = 0x1000, kHeader = 28;
uint32_t le32(const uint8_t* p) { return (uint32_t)p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24; }
uint16_t le16(const uint8_t* p) { return (uint16_t)(p[0] | p[1] << 8); }
}  // namespace

int SaveStream::bit() {
    if (mask_ == 0) {
        if (pos_ >= d_.size()) throw FormatError("save stream ends");
        cur_ = d_[pos_++];
        mask_ = 0x80;
    }
    const int b = (cur_ & mask_) ? 1 : 0;
    mask_ >>= 1;
    return b;
}

uint32_t SaveStream::bits(int n) {
    uint32_t v = 0;
    for (int i = 0; i < n; i++) v = v << 1 | (uint32_t)bit();
    return v;
}

int32_t SaveStream::packed(int size) {
    if (!bit()) return 0;
    const int code = (int)bits(2);
    const int w = (size == 8 ? kWidths8 : size == 16 ? kWidths16 : kWidths32)[code];
    const uint32_t v = bits(w);
    if (w < 32 && (v & (1u << (w - 1)))) return (int32_t)(v - (1u << w));
    return (int32_t)v;
}

uint32_t SaveStream::raw(int n) {
    mask_ = 0;
    if (pos_ + (size_t)n > d_.size()) throw FormatError("save stream ends");
    uint32_t v = 0;
    for (int i = 0; i < n; i++) v |= (uint32_t)d_[pos_ + (size_t)i] << (8 * i);
    pos_ += (size_t)n;
    return v;
}

int32_t SaveStream::r8() { return compressed ? packed(8) : (int8_t)raw(1); }
int32_t SaveStream::r16() { return compressed ? packed(16) : (int16_t)raw(2); }
int32_t SaveStream::r32() { return compressed ? packed(32) : (int32_t)raw(4); }
float SaveStream::rfloat() {
    const uint32_t u = (uint32_t)r32();
    float f;
    std::memcpy(&f, &u, 4);
    return f;
}
uint32_t SaveStream::mark() { return raw(4); }

std::vector<uint8_t> unpack_lzss(std::span<const uint8_t> src, size_t size, size_t* used) {
    uint8_t ring[4096] = {};
    size_t r = 0xFEE, i = 0;
    unsigned flags = 0;
    std::vector<uint8_t> out;
    out.reserve(size);
    while (out.size() < size) {
        flags >>= 1;
        if (!(flags & 0x100)) {
            if (i >= src.size()) throw FormatError("packed block ends early");
            flags = src[i++] | 0xFF00u;
        }
        if (flags & 1) {
            if (i >= src.size()) throw FormatError("packed block ends early");
            const uint8_t c = src[i++];
            out.push_back(c);
            ring[r] = c;
            r = (r + 1) & 4095;
        } else {
            if (i + 1 >= src.size()) throw FormatError("packed block ends early");
            const unsigned lo = src[i], hi = src[i + 1];
            i += 2;
            const size_t pos = lo | (hi & 0xF0) << 4;
            for (unsigned k = 0; k < (hi & 0x0F) + 3 && out.size() < size; k++) {
                const uint8_t c = ring[(pos + k) & 4095];
                out.push_back(c);
                ring[r] = c;
                r = (r + 1) & 4095;
            }
        }
    }
    if (used) *used = i;
    return out;
}

std::vector<NeighbourhoodBlock> read_neighbourhood(std::span<const uint8_t> f) {
    std::vector<NeighbourhoodBlock> out;
    for (size_t off = 0; off + kHeader <= f.size(); off += kBlock) {
        const uint8_t* h = f.data() + off;
        const uint32_t section = le32(h), hs = le32(h + 4), un = le32(h + 8), pk = le32(h + 12), four = le32(h + 20);
        if (hs != kHeader || four != 4 || pk == 0 || pk > un || un >= (1u << 24) || off + kHeader + pk > f.size()) continue;
        size_t used = 0;
        NeighbourhoodBlock b{(uint32_t)off, section, le32(h + 24), unpack_lzss(f.subspan(off + kHeader, pk), un, &used)};
        if (used != pk) throw FormatError("a block does not unpack from exactly its packed size");
        out.push_back(std::move(b));
    }
    return out;
}

const std::vector<uint8_t>* LotChunks::find(const std::string& type) const {
    for (const auto& [t, c] : chunks) if (t == type) return &c;
    return nullptr;
}

std::vector<LotChunks> read_lots(std::span<const uint8_t> file) {
    std::vector<LotChunks> out;
    for (auto& b : read_neighbourhood(file)) {
        if (b.section != 3) continue;
        LotChunks lot;
        lot.block = b.offset;
        const auto& d = b.data;
        size_t n = 0;
        auto alpha = [](uint8_t c) { return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z'); };
        while (4 * n + 4 <= d.size() && alpha(d[4 * n]) && alpha(d[4 * n + 1]) && alpha(d[4 * n + 2]) && alpha(d[4 * n + 3])) n++;
        if (12 * n > d.size()) throw FormatError("a lot's chunk table runs past its block");
        for (size_t k = 0; k < n; k++) {
            std::string tag((const char*)d.data() + 4 * k, 4);
            std::string type(tag.rbegin(), tag.rend());  // stored backwards
            const uint32_t at = le32(d.data() + 4 * n + 4 * k), size = le32(d.data() + 8 * n + 4 * k);
            if (at < kHeader || at - kHeader + size > d.size()) throw FormatError("a lot's chunk runs past its block");
            lot.chunks.push_back({type, std::vector<uint8_t>(d.begin() + (at - kHeader), d.begin() + (at - kHeader + size))});
        }
        out.push_back(std::move(lot));
    }
    return out;
}

std::vector<ObjectType> read_object_types(std::span<const uint8_t> c) {
    if (c.size() < 12 || std::memcmp(c.data() + 8, "tjbo", 4) != 0) throw FormatError("not an objt");
    std::vector<ObjectType> out;
    size_t p = 12;
    while (true) {
        if (p + 4 > c.size()) throw FormatError("objt has no end");
        const uint32_t guid = le32(c.data() + p);
        if (guid == 0) {
            if (p + 4 != c.size()) throw FormatError("objt does not end at its zero GUID");
            return out;
        }
        if (p + 16 > c.size()) throw FormatError("objt entry runs past the chunk");
        ObjectType t;
        t.guid = guid;
        for (int k = 0; k < 6; k++) t.fields[k] = le16(c.data() + p + 4 + 2 * k);
        size_t q = p + 16;
        while (q < c.size() && c[q]) q++;
        if (q >= c.size()) throw FormatError("objt name has no end");
        t.name.assign((const char*)c.data() + p + 16, q - p - 16);
        p = q + 1;
        if ((p - 12) % 2) p++;
        out.push_back(std::move(t));
    }
}

LotObjects read_lot_objects(std::span<const uint8_t> m) {
    constexpr size_t kStart = 12;
    if (m.size() < kStart + 1 || std::memcmp(m.data() + 8, "MjbO", 4) != 0) throw FormatError("not an ObjM");
    const auto s = m.subspan(kStart);
    SaveStream r(s);
    r.compressed = r.rbool();
    LotObjects out;
    while (true) {
        LotObject o;
        o.id = (int16_t)r.r16();
        if (o.id == 0) break;
        o.a = r.r32();
        o.b = r.r32();
        const int n = r.r8() & 0xFF;
        for (int k = 0; k < n; k++) o.extra.push_back(r.r32());
        o.type = (int16_t)r.r16();
        out.objects.push_back(std::move(o));
    }
    // The mark chain: each mark the position of the next; one section per object, then the
    // lot's own part to the stream's end.
    size_t mark_at = r.pos();
    uint32_t next = r.mark();
    std::vector<std::pair<size_t, size_t>> sections;
    while (true) {
        const size_t start = mark_at + 4;
        if (next <= mark_at || next + 4 > s.size()) { sections.push_back({start, s.size()}); break; }
        sections.push_back({start, next});
        mark_at = next;
        next = le32(s.data() + next);
    }
    if (sections.size() != out.objects.size() + 1) throw FormatError("ObjM: not one section per object");
    for (size_t k = 0; k < out.objects.size(); k++) {
        LotObject& o = out.objects[k];
        o.section = kStart + sections[k].first;
        o.section_end = kStart + sections[k].second;
        SaveStream h(s, sections[k].first);
        h.compressed = true;
        for (int i = 0; i < 6; i++) o.header[i] = h.r32();
        o.header[6] = h.r32();
        o.header[7] = h.r16();
        const int n = h.r16();
        if (n < 0 || n > 4096) throw FormatError("ObjM: an object's attribute count is out of range");
        for (int i = 0; i < n; i++) o.attributes.push_back((int16_t)h.r16());
        for (int i = 0; i < kObjectTemps; i++) o.temps.push_back((int16_t)h.r16());
        for (int i = 0; i < kObjectDataFields; i++) o.data.push_back((int16_t)h.r16());
        if (o.data[11] != o.id) throw FormatError("ObjM: an object's data 11 is not its id");
    }
    out.lot_section = kStart + sections.back().first;
    out.lot_section_end = kStart + sections.back().second;
    return out;
}

}  // namespace housefire
