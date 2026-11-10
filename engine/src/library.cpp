#include <algorithm>
#include "housefire/library.h"
#include "housefire/save.h"

#include <cctype>

namespace housefire {

namespace {
std::string lower(std::string s) {
    for (char& c : s) c = (char)std::tolower((unsigned char)c);
    return s;
}
}  // namespace

std::string glob_name(std::span<const uint8_t> d) {
    if (d.empty()) return {};
    if (d[0] < 0x20) return d[0] + 1u <= d.size() ? std::string((const char*)d.data() + 1, d[0]) : std::string();
    size_t n = 0;
    while (n < d.size() && d[n]) n++;
    return std::string((const char*)d.data(), n);
}

namespace {
// A string table (STR#): its format (0 Pascal strings, -1 C strings, -2 pairs of C strings,
// -3 a language, a value and a comment, -4 sets by language of Pascal values and comments),
// then the strings; the values, each language's in turn. A table that does not read is empty.
std::vector<std::string> read_strings(std::span<const uint8_t> d, Order o) {
    std::vector<std::string> out;
    size_t p = 2;
    auto cstr = [&] {
        size_t q = p;
        while (q < d.size() && d[q]) q++;
        std::string s((const char*)d.data() + p, std::min(q, d.size()) - p);
        p = std::min(q + 1, d.size());
        return s;
    };
    auto pstr = [&] {
        if (p >= d.size()) return std::string();
        const size_t n = d[p++];
        std::string s((const char*)d.data() + p, std::min(n, d.size() - p));
        p += n;
        return s;
    };
    auto u16 = [&] {
        if (p + 2 > d.size()) return 0;
        const int v = o == Order::Little ? d[p] | d[p + 1] << 8 : d[p] << 8 | d[p + 1];
        p += 2;
        return v;
    };
    if (d.size() < 4) return out;
    const int16_t fmt = (int16_t)(o == Order::Little ? d[0] | d[1] << 8 : d[0] << 8 | d[1]);
    if (fmt == -4) {
        const int sets = p < d.size() ? d[p++] : 0;
        for (int s = 0; s < sets; s++)
            for (int n = u16(); n > 0 && p < d.size(); n--) { p++; out.push_back(pstr()); pstr(); }
        return out;
    }
    for (int n = u16(); n > 0 && p < d.size(); n--) {
        if (fmt == 0) out.push_back(pstr());
        else if (fmt == -1) out.push_back(cstr());
        else if (fmt == -2) { out.push_back(cstr()); cstr(); }
        else if (fmt == -3) { p++; out.push_back(cstr()); cstr(); }
        else break;
    }
    return out;
}
// An interaction table (TTAB): each interaction's action and guard trees (docs/interpreter.md;
// tools/chunks.py reads the rest). Versions below 9 are plain fields in the file's order;
// 9 and up are packed as the save stream is (a code byte 1, then its compressed fields; 11
// opens with one field for the table). A table that does not read adds nothing.
void read_ttab(std::span<const uint8_t> d, Order o, std::map<uint16_t, uint16_t>& out, std::map<uint16_t, uint16_t>& prefix) {
    try {
        Reader r(d, o);
        const uint16_t n = r.u16();
        if (n == 0 || n > 4096) return;
        const uint16_t version = r.u16();
        if (version < 9) {
            for (int i = 0; i < n; i++) {
                const uint16_t action = r.u16(), guard = r.u16();
                uint32_t motives;
                if (version <= 3) { motives = r.u16(); r.u16(); }
                else { motives = r.u32(); r.u32(); r.u32(); }
                if (motives > 64) return;
                if (version > 6) r.u32();
                r.u32();
                if (version > 3) { r.u32(); r.u32(); }
                r.take((version > 6 ? 6 : 2) * motives);
                if (guard) out.emplace(action, guard);
            }
            return;
        }
        if (r.u8() != 1) return;
        SaveStream f(d.subspan(r.pos()));
        f.compressed = true;
        const uint16_t table = version >= 11 ? (uint16_t)f.r16() : 0;
        for (int i = 0; i < n; i++) {
            const uint16_t action = (uint16_t)f.r16(), guard = (uint16_t)f.r16();
            if (table) prefix.emplace(action, table);
            const int32_t motives = f.r32();
            if (motives < 0 || motives > 64) return;
            for (int k = 0; k < 6; k++) f.r32();  // flags, string index, attenuation, threshold, joining
            for (int32_t k = 0; k < motives; k++) { f.r16(); f.r16(); f.r16(); }
            if (guard) out.emplace(action, guard);
        }
    } catch (const FormatError&) {
    }
}

}  // namespace

Library Library::bustin_out(const Disc& d) {
    Library lib;
    for (const auto& [path, where] : d.files()) {
        if (path.rfind("Runtime/", 0) != 0 || path.size() < 5 || lower(path.substr(path.size() - 4)) != ".iff") continue;
        auto f = std::make_unique<ObjectFile>();
        f->path = path;
        f->bytes = d.read(path);
        f->iff = read_iff(f->bytes);
        const Order o = bhav_order(f->iff);
        for (const Chunk* c : f->iff.of("BHAV")) {
            try { f->trees[(uint16_t)c->id] = read_bhav(*c, o); } catch (const FormatError&) { lib.bhav_failures_++; }
            f->tree_by_name.emplace(c->label, (uint16_t)c->id);
        }
        for (const Chunk* c : f->iff.of("STR#")) f->strings[(uint16_t)c->id] = read_strings(c->data, o);
        for (const Chunk* c : f->iff.of("TTAB")) read_ttab(c->data, o, f->guard_of, f->prefix_of);
        for (const Chunk* c : f->iff.of("BCON")) {
            // A count, a flags byte, then the values.
            if (c->data.size() < 2) continue;
            Reader r(c->data, o);
            const uint8_t n = r.u8();
            r.u8();
            std::vector<int16_t> vals;
            for (int k = 0; k < n && r.left() >= 2; k++) vals.push_back(r.s16());
            f->constants[(uint16_t)c->id] = std::move(vals);
        }
        for (const Chunk* c : f->iff.of("GLOB")) f->glob = glob_name(c->data);
        for (const Chunk* c : f->iff.of("OBJD")) {
            Reader r(c->data, o);
            Definition d;
            d.id = c->id;
            if (r.left() < 4) continue;
            d.version = r.u32();
            while (r.left() >= 2) d.fields.push_back(r.u16());
            f->definitions.push_back(std::move(d));
        }
        const size_t slash = path.rfind('/');
        lib.by_name_.emplace(lower(path.substr(slash + 1, path.size() - slash - 5)), f.get());  // the first of a name
        lib.files_.push_back(std::move(f));
    }
    for (auto& f : lib.files_) {
        std::string g = lower(f->glob);
        if (g == "personglobals") g = "person_sg";
        if (!g.empty()) f->semi = lib.find(g);
    }
    lib.global_ = lib.find("global");
    for (const auto& f : lib.files_)
        for (size_t k = 0; k < f->definitions.size(); k++)
            lib.by_guid_.emplace(f->definitions[k].guid(), std::make_pair(f.get(), k));
    return lib;
}

const ObjectFile* Library::find(const std::string& name) const {
    auto it = by_name_.find(lower(name));
    return it == by_name_.end() ? nullptr : it->second;
}

const Tree* Library::tree(const ObjectFile* object, uint16_t id) const {
    const ObjectFile* f = nullptr;
    switch (tree_kind(id)) {
        case TreeKind::Global: f = global_; break;
        case TreeKind::Own: f = object; break;
        case TreeKind::SemiGlobal: f = object ? object->semi : nullptr; break;
        default: return nullptr;
    }
    if (!f) return nullptr;
    auto it = f->trees.find(id);
    return it == f->trees.end() ? nullptr : &it->second;
}

std::optional<int16_t> Library::constant(const ObjectFile* object, int16_t data) const {
    const int table = (uint16_t)data >> 7, entry = data & 0x7F;
    const ObjectFile* f = table < 64 ? object : table < 128 ? (object ? object->semi : nullptr) : global_;
    const uint16_t id = (uint16_t)(table < 64 ? 4096 + table : table < 128 ? 8192 + table - 64 : 256 + table - 128);
    if (!f) return std::nullopt;
    auto t = f->constants.find(id);
    if (t == f->constants.end() || entry >= (int)t->second.size()) return std::nullopt;
    return t->second[entry];
}

const Definition* Library::definition(uint32_t guid, const ObjectFile** file) const {
    auto it = by_guid_.find(guid);
    if (it == by_guid_.end()) return nullptr;
    if (file) *file = it->second.first;
    return &it->second.first->definitions[it->second.second];
}

}  // namespace housefire
