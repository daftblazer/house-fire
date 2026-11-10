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
        // The sixth word is the format's version: Bustin' Out's 4, Castaway's 11.
        const uint32_t section = le32(h), hs = le32(h + 4), un = le32(h + 8), pk = le32(h + 12), version = le32(h + 20);
        if (hs != kHeader || version == 0 || version >= 64 || pk == 0 || pk > un || un >= (1u << 24) || off + kHeader + pk > f.size()) continue;
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

namespace {
// A block's chunk table (a lot's, the neighbourhood's): n FourCCs (backwards), n offsets from
// the block's start in the file (header included), n sizes.
LotChunks chunk_table(const NeighbourhoodBlock& b) {
    LotChunks lot;
    lot.block = b.offset;
    const auto& d = b.data;
    size_t n = 0;
    auto alpha = [](uint8_t c) { return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z'); };
    while (4 * n + 4 <= d.size() && alpha(d[4 * n]) && alpha(d[4 * n + 1]) && alpha(d[4 * n + 2]) && alpha(d[4 * n + 3])) n++;
    if (12 * n > d.size()) throw FormatError("a chunk table runs past its block");
    for (size_t k = 0; k < n; k++) {
        std::string tag((const char*)d.data() + 4 * k, 4);
        std::string type(tag.rbegin(), tag.rend());  // stored backwards
        const uint32_t at = le32(d.data() + 4 * n + 4 * k), size = le32(d.data() + 8 * n + 4 * k);
        if (at < kHeader || at - kHeader + size > d.size()) throw FormatError("a chunk runs past its block");
        lot.chunks.push_back({type, std::vector<uint8_t>(d.begin() + (at - kHeader), d.begin() + (at - kHeader + size))});
    }
    return lot;
}
}  // namespace

std::vector<LotChunks> read_lots(std::span<const uint8_t> file) {
    std::vector<LotChunks> out;
    for (auto& b : read_neighbourhood(file))
        if (b.section == 3) out.push_back(chunk_table(b));
    return out;
}

std::vector<Neighbour> read_neighbours(std::span<const uint8_t> file) {
    const std::vector<uint8_t>* nbrs = nullptr;
    LotChunks t;
    for (auto& b : read_neighbourhood(file))
        if (b.section == 1) { t = chunk_table(b); nbrs = t.find("NBRS"); break; }
    if (!nbrs) throw FormatError("the neighbourhood has no NBRS");
    const std::vector<uint8_t>& c = *nbrs;
    size_t p = 0;
    auto need = [&](size_t n) { if (p + n > c.size()) throw FormatError("NBRS runs past its chunk"); };
    auto i32 = [&] { need(4); const int32_t v = (int32_t)le32(c.data() + p); p += 4; return v; };
    auto i16 = [&] { need(2); const int16_t v = (int16_t)le16(c.data() + p); p += 2; return v; };
    need(16);
    if (std::memcmp(c.data() + 8, "SRBN", 4) != 0) throw FormatError("not an NBRS");
    p = 12;
    const int32_t slots = i32();
    if (slots < 0 || slots > 4096) throw FormatError("NBRS: slots out of range");
    std::vector<Neighbour> out;
    for (int32_t s = 0; s < slots; s++) {
        if (i32() == 0) continue;  // an empty slot
        Neighbour n;
        i32();  // the record's version (4)
        size_t q = p;
        while (q < c.size() && c[q]) q++;
        if (q >= c.size()) throw FormatError("NBRS: a name has no end");
        n.name.assign((const char*)c.data() + p, q - p);
        p = q + 1;
        if (p & 1) p++;
        i32();
        const int32_t mode = i32();  // set: the person data follows
        if (mode)
            for (int k = 0; k < 80; k++) n.person_data.push_back(i16());
        need(128);
        p += 128;  // to be named (floats among them)
        n.id = i16();
        n.guid = (uint32_t)i32();
        i32();
        i32();
        const int32_t rels = i32();
        if (rels < 0 || rels > 4096) throw FormatError("NBRS: relationships out of range");
        for (int32_t r = 0; r < rels; r++) {
            i32();
            Neighbour::Relationship rel;
            rel.with = (int16_t)i32();
            const int32_t k = i32();
            if (k < 0 || k > 64) throw FormatError("NBRS: a relationship's values out of range");
            for (int32_t j = 0; j < k; j++) rel.values.push_back(i32());
            n.relationships.push_back(std::move(rel));
        }
        out.push_back(std::move(n));
    }
    if (p != c.size()) throw FormatError("NBRS does not end at its chunk's end");
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

namespace {

constexpr int kVersion = 80;  // ObjM's version on Bustin' Out's disc; the conditions are its loader's

int32_t count(SaveStream& r, int32_t limit, const char* what) {
    const int32_t n = r.r32();
    if (n < 0 || n > limit) throw FormatError(what);
    return n;
}

// The +192 member (fn_8023243C, its list fn_80232E2C, entries fn_802326CC / fn_80233084):
// read past.
void skip_slot_entry(SaveStream& r) {
    r.r32();
    for (int32_t n = count(r, 4096, "ObjM: a slot list out of range"); n > 0; n--) r.r32();
}

void skip_slots(SaveStream& r) {
    const int32_t a = r.r32();
    if (a >= 0) {
        if (a > 4096) throw FormatError("ObjM: slots out of range");
        for (int32_t i = 0; i < a; i++) skip_slot_entry(r);
        return;
    }
    if (kVersion > 78) r.r32();
    for (int32_t n = count(r, 4096, "ObjM: slot entries out of range"); n > 0; n--)
        if (r.r32()) skip_slot_entry(r);  // an int: the entry is there
}

// After the data: a zero, the tree stack (fn_8023F928, frames fn_8023E9A8, file
// fn_801F961C), the +192 member, the pairs (fn_801F22D0), a 32-bit value, the shorts.
void read_base_rest(SaveStream& r, LotObject& o) {
    r.r16();
    const int32_t frames = r.r32();
    if (frames < 0 || frames > 64) throw FormatError("ObjM: a tree stack out of range");
    r.r32();  // its capacity
    for (int32_t i = 0; i < frames; i++) {
        SavedFrame f;
        f.stack_object = (int16_t)r.r16();
        f.tree = (uint16_t)r.r16();
        f.node = (uint16_t)r.r16();
        const int nlocals = r.r8() & 0xFF, nparams = r.r8() & 0xFF;
        for (int k = 0; k < nparams; k++) f.params.push_back((int16_t)r.r16());
        for (int k = 0; k < nlocals; k++) f.locals.push_back((int16_t)r.r16());
        f.engine = r.r32() != 0;
        r.r16();  // the original's file index
        o.stack.push_back(std::move(f));
    }
    skip_slots(r);
    for (int n = r.r16(); n > 0; n--) {
        const int16_t a = (int16_t)r.r16();
        o.pairs.push_back({a, (int16_t)r.r16()});
    }
    r.r32();
    for (int n = r.r16(); n > 0; n--) o.shorts.push_back((int16_t)r.r16());
}

void no_elements(SaveStream& r, const char* what) {
    if (r.r32() != 0) throw FormatError(what);  // never filled on the disc: not read yet
}

SavedQueueEntry read_queue_entry(SaveStream& r) {
    SavedQueueEntry e;
    e.a = r.r32();
    e.id = r.r32();
    for (int16_t& o : e.objects) o = (int16_t)r.r16();  // fn_801FDD0C
    e.b = r.r32();
    for (int16_t& v : e.shorts) v = (int16_t)r.r16();
    e.c = r.r32();
    e.d = (int16_t)r.r16();
    e.e = r.rfloat();
    e.flags = r.r32();  // from version 62
    return e;
}

// A Sim's own part (fn_80224C88), for version 80.
void read_sim(SaveStream& r, LotObject& o) {
    // Its +1136 member (fn_800AD6E8): a count of (bool, 32-bit) pairs, five values.
    for (int n = r.r16(); n > 0; n--) { r.rbool(); r.r32(); }
    r.rbool(); r.rfloat(); r.r32(); r.r32(); r.r32();
    for (int i = 0; i < 16; i++) o.motives.push_back(r.rfloat());
    for (int i = 0; i < 16; i++) o.motives_b.push_back(r.rfloat());
    for (int i = 0; i < 80; i++) o.person_data.push_back((int16_t)r.r16());
    no_elements(r, "ObjM: a Sim's +1160 list is not empty");
    o.current = read_queue_entry(r);
    const int32_t n = r.r32();
    if (n < 0 || n > 8) throw FormatError("ObjM: a Sim's queue out of range");
    for (int32_t i = 0; i < n; i++) o.queue.push_back(read_queue_entry(r));
    no_elements(r, "ObjM: a Sim's +1180 list is not empty");
    no_elements(r, "ObjM: a Sim's +1120 list is not empty");
}

// A multi-tile object's part (fn_801D3F50) and a door's (fn_8022DE18): its leader's id kept,
// the rest read past (its tile offsets among it).
void read_part(SaveStream& r, LotObject& o, bool door) {
    for (int i = 0; i < 6; i++) r.r32();
    o.leader = (int16_t)r.r16();
    if (!door) return;
    const int32_t n = count(r, 4096, "ObjM: a door's values out of range");
    for (int32_t i = 0; i < n; i++) r.rfloat();
}

}  // namespace

LotObjects read_lot_objects(std::span<const uint8_t> m, const std::function<SavedClass(int16_t type)>& class_of) {
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
        read_base_rest(h, o);
        if (h.pos() > sections[k].second) throw FormatError("ObjM: an object runs past its section");
        o.base_end = kStart + h.pos();
        o.saved_class = class_of(o.type);
        switch (o.saved_class) {
            case SavedClass::Object: break;
            case SavedClass::Part: read_part(h, o, false); break;
            case SavedClass::Door: read_part(h, o, true); break;
            case SavedClass::Sim: read_sim(h, o); break;
        }
        h.r32();  // the loader's int
        h.pad();
        if (h.pos() != sections[k].second) throw FormatError("ObjM: an object's section does not end at its mark");
    }
    out.lot_section = kStart + sections.back().first;
    out.lot_section_end = kStart + sections.back().second;
    {
        SaveStream l(s, sections.back().first);
        l.compressed = true;
        out.lot_object = (int16_t)l.r16();  // the manager's object reference (fn_801FDD0C)
        const int32_t n = count(l, 4096, "ObjM: the lot's objects out of range");
        for (int32_t i = 0; i < n; i++) out.lot_objects.push_back((int16_t)l.r16());
        l.pad();  // the string (Recon_String2): UTF-16 from an even position
        size_t p = l.pos() + (l.pos() & 1);
        bool little = false;
        if (p + 2 <= s.size() && s[p] == 0xFE && s[p + 1] == 0xFF) { little = true; p += 2; }
        else if (p + 2 <= s.size() && s[p] == 0xFF && s[p + 1] == 0xFE) p += 2;
        while (true) {
            if (p + 2 > s.size()) throw FormatError("ObjM: the lot's string has no end");
            const char16_t u = little ? (char16_t)le16(s.data() + p) : (char16_t)(s[p] << 8 | s[p + 1]);
            p += 2;
            if (!u) break;
            out.lot_string.push_back(u);
        }
        if (p != s.size()) throw FormatError("ObjM: the lot's part does not end the stream");
    }
    return out;
}

}  // namespace housefire
