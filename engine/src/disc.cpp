#include "housefire/disc.h"

#include <cstring>
#include <sys/types.h>

namespace housefire {

namespace {
uint32_t be32(const uint8_t* p) { return (uint32_t)p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]; }

bool read_at(std::FILE* f, uint64_t off, void* dst, size_t n) {
    return fseeko(f, (off_t)off, SEEK_SET) == 0 && fread(dst, 1, n, f) == n;
}
}  // namespace

Disc::Disc(const std::string& path) {
    f_ = std::fopen(path.c_str(), "rb");
    if (!f_) return;
    uint8_t h[0x440];
    if (!read_at(f_, 0, h, sizeof h)) { std::fclose(f_); f_ = nullptr; return; }
    id_.assign((const char*)h, 6);
    const uint32_t fst_off = be32(h + 0x424), fst_size = be32(h + 0x428);
    if (fst_size < 12 || fst_size > (64u << 20)) { std::fclose(f_); f_ = nullptr; return; }
    std::vector<uint8_t> fst(fst_size);
    if (!read_at(f_, fst_off, fst.data(), fst_size)) { std::fclose(f_); f_ = nullptr; return; }
    const uint32_t n = be32(fst.data() + 8);
    if ((uint64_t)n * 12 > fst_size) { std::fclose(f_); f_ = nullptr; return; }
    const char* names = (const char*)fst.data() + n * 12;
    const size_t names_size = fst_size - n * 12;
    std::vector<std::pair<uint32_t, std::string>> dirs = {{n, ""}};  // (end, path)
    for (uint32_t i = 1; i < n; i++) {
        while (!dirs.empty() && i >= dirs.back().first) dirs.pop_back();
        const uint8_t* e = fst.data() + i * 12;
        const uint32_t name_off = (uint32_t)e[1] << 16 | e[2] << 8 | e[3];
        if (name_off >= names_size) break;
        std::string name(names + name_off, strnlen(names + name_off, names_size - name_off));
        const std::string p = (dirs.empty() ? std::string() : dirs.back().second) + name;
        if (e[0]) dirs.push_back({be32(e + 8), p + "/"});
        else files_[p] = {be32(e + 4), be32(e + 8)};
    }
}

Disc::~Disc() {
    if (f_) std::fclose(f_);
}

std::vector<uint8_t> Disc::read(const std::string& path) const {
    auto it = files_.find(path);
    if (it == files_.end() || !f_) return {};
    std::vector<uint8_t> b(it->second.second);
    if (!read_at(f_, it->second.first, b.data(), b.size())) return {};
    return b;
}

}  // namespace housefire
