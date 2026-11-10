#include <cstring>
#include <algorithm>
#include "housefire/library.h"
#include "housefire/world.h"
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
// A tuning table (FCNS): 0, 1, "SNCF", the entry count, then each entry a name, a float and a
// description, the strings zero-ended and padded to an even length (with 0xCD). Empty if the
// chunk does not read to exactly its end.
std::vector<std::pair<std::string, float>> read_tuning(std::span<const uint8_t> d, Order o) {
    std::vector<std::pair<std::string, float>> out;
    auto u32 = [&](size_t p) {
        return o == Order::Little ? (uint32_t)d[p] | d[p + 1] << 8 | d[p + 2] << 16 | (uint32_t)d[p + 3] << 24
                                  : (uint32_t)d[p] << 24 | d[p + 1] << 16 | d[p + 2] << 8 | d[p + 3];
    };
    if (d.size() < 16 || std::memcmp(d.data() + 8, "SNCF", 4) != 0) return out;
    const uint32_t count = u32(12);
    size_t p = 16;
    auto zstring = [&](std::string& s) {
        size_t q = p;
        while (q < d.size() && d[q]) q++;
        if (q >= d.size()) return false;
        s.assign((const char*)d.data() + p, q - p);
        p = q + 1;
        p += p & 1;
        return true;
    };
    for (uint32_t i = 0; i < count; i++) {
        std::string name, description;
        if (!zstring(name) || p + 4 > d.size()) return {};
        const uint32_t w = u32(p);
        float v;
        std::memcpy(&v, &w, 4);
        p += 4;
        if (!zstring(description)) return {};
        out.emplace_back(std::move(name), v);
    }
    if (p != d.size()) return {};
    return out;
}

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
void read_ttab(std::span<const uint8_t> d, Order o, std::map<uint16_t, uint16_t>& out, std::map<uint16_t, uint16_t>& prefix,
               std::vector<std::pair<uint16_t, uint16_t>>& list, std::vector<Interaction>& table_out, uint16_t& first) {
    try {
        Reader r(d, o);
        const uint16_t n = r.u16();
        if (n == 0 || n > 4096) return;
        const uint16_t version = r.u16();
        if (version < 9) {
            for (int i = 0; i < n; i++) {
                Interaction e;
                e.action = r.u16();
                e.guard = r.u16();
                const uint16_t action = e.action, guard = e.guard;
                uint32_t motives;
                if (version <= 3) { motives = r.u16(); e.flags = r.u16(); }
                else { motives = r.u32(); e.flags = r.u32(); e.string_index = (int32_t)r.u32(); }
                if (motives > 64) return;
                if (version > 6) e.attenuation_code = (int32_t)r.u32();
                { const uint32_t w = r.u32(); std::memcpy(&e.attenuation_value, &w, 4); }
                if (version > 3) { e.autonomy_threshold = (int32_t)r.u32(); e.joining_index = (int32_t)r.u32(); }
                for (uint32_t k = 0; k < motives; k++) {
                    Interaction::Advert a;
                    if (version > 6) { a.min = r.s16(); a.delta = r.s16(); a.personality = r.u16(); }
                    else a.delta = r.s16();
                    e.adverts.push_back(a);
                }
                table_out.push_back(std::move(e));
                list.push_back({action, guard});
                if (guard) out.emplace(action, guard);
            }
            return;
        }
        if (r.u8() != 1) return;
        SaveStream f(d.subspan(r.pos()));
        f.compressed = true;
        const uint16_t table = version >= 11 ? (uint16_t)f.r16() : 0;
        first = table;
        for (int i = 0; i < n; i++) {
            Interaction e;
            e.action = (uint16_t)f.r16();
            e.guard = (uint16_t)f.r16();
            const uint16_t action = e.action, guard = e.guard;
            if (table) prefix.emplace(action, table);
            const int32_t motives = f.r32();
            if (motives < 0 || motives > 64) return;
            e.flags = (uint32_t)f.r32();
            e.string_index = f.r32();
            e.attenuation_code = f.r32();
            { const int32_t w = f.r32(); std::memcpy(&e.attenuation_value, &w, 4); }
            e.autonomy_threshold = f.r32();
            e.joining_index = f.r32();
            for (int32_t k = 0; k < motives; k++) {
                Interaction::Advert a;
                a.min = (int16_t)f.r16();
                a.delta = (int16_t)f.r16();
                a.personality = (uint16_t)f.r16();
                e.adverts.push_back(a);
            }
            table_out.push_back(std::move(e));
            list.push_back({action, guard});
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
        for (const Chunk* c : f->iff.of("FCNS")) {
            auto t = read_tuning(c->data, o);
            if (t.empty()) lib.tuning_failures_++;
            else f->tuning[(uint16_t)c->id] = std::move(t);
        }
        for (const Chunk* c : f->iff.of("TTAB")) {
            std::vector<std::pair<uint16_t, uint16_t>> list;
            std::vector<Interaction> table;
            uint16_t first = 0;
            read_ttab(c->data, o, f->guard_of, f->prefix_of, list, table, first);
            if (!table.empty()) f->tables[(uint16_t)c->id] = std::move(table);
            if (first) f->table_first[(uint16_t)c->id] = first;
            if (f->ttabs++ == 0) f->interactions = std::move(list);
            else f->interactions.clear();  // several tables: which an object uses is not known yet
        }
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

std::optional<float> Library::tuning(std::string_view name) const {
    if (!global_) return std::nullopt;
    for (const auto& [id, table] : global_->tuning)
        for (const auto& [n, v] : table)
            if (n == name) return v;
    return std::nullopt;
}

const std::vector<Interaction>* Library::interactions_of(const Definition& d, const ObjectFile& file) const {
    uint16_t id = d.interaction_table();
    if (id == 0xFFFF && d.field(8))
        for (const Definition& m : file.definitions)
            if (m.field(8) == d.field(8) && m.interaction_table() != 0xFFFF) { id = m.interaction_table(); break; }
    if (id == 0xFFFF && file.tables.size() == 1) return &file.tables.begin()->second;
    auto it = file.tables.find(id);
    return it == file.tables.end() ? nullptr : &it->second;
}

Library::ObjectTable Library::table_of(const Object& o) const {
    ObjectTable t;
    if (!o.file) return t;
    if (o.person) {
        t.file = o.file->semi;
        if (t.file && t.file->tables.size() == 1) t.table = &t.file->tables.begin()->second;
    } else if (o.definition) {
        t.file = o.file;
        t.table = interactions_of(*o.definition, *o.file);
    }
    if (t.table && t.file)
        for (const auto& [id, tab] : t.file->tables)
            if (&tab == t.table) {
                auto f = t.file->table_first.find(id);
                if (f != t.file->table_first.end()) t.first = f->second;
            }
    return t;
}

}  // namespace housefire
