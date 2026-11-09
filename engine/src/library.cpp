#include "housefire/library.h"

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

}  // namespace housefire
