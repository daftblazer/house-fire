// Files from a GameCube disc image (.iso, read in place; tools/disc.py reads the others).
#pragma once
#include <cstdint>
#include <cstdio>
#include <map>
#include <string>
#include <vector>

namespace housefire {

class Disc {
public:
    explicit Disc(const std::string& path);
    ~Disc();
    Disc(const Disc&) = delete;
    Disc& operator=(const Disc&) = delete;

    bool ok() const { return f_ != nullptr; }
    const std::string& id() const { return id_; }
    // path -> (offset, size), paths as on the disc ("Runtime/GameData/Behavior.iff").
    const std::map<std::string, std::pair<uint32_t, uint32_t>>& files() const { return files_; }
    std::vector<uint8_t> read(const std::string& path) const;

private:
    std::FILE* f_ = nullptr;
    std::string id_;
    std::map<std::string, std::pair<uint32_t, uint32_t>> files_;
};

}  // namespace housefire
