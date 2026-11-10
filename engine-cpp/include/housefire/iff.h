// Maxis IFF 2.5, as the GameCube Sims games keep it (docs/survey.md, tools/iff.py).
#pragma once
#include <cstdint>
#include <span>
#include <string>
#include <vector>

namespace housefire {

enum class Order { Little, Big };  // a chunk's contents: Bustin' Out keeps the PC's, v3.0 big-endian

struct Chunk {
    char type[5] = {};
    int16_t id = 0;
    uint16_t flags = 0;
    std::string label;
    std::span<const uint8_t> data;  // the contents, header excluded
};

// The chunks of the IFF file at the start of `file`. Stops at the first chunk header that is
// not one (the next file in an archive, or padding); `ok` is false if `file` is not an IFF.
struct IffFile {
    bool ok = false;
    std::vector<Chunk> chunks;
    std::vector<const Chunk*> of(const char* type) const;
    const Chunk* find(const char* type, int16_t id) const;
};
IffFile read_iff(std::span<const uint8_t> file);

// Little or big-endian reads within a span; a read past the end throws FormatError.
struct FormatError : std::exception {
    std::string what_;
    explicit FormatError(std::string w) : what_(std::move(w)) {}
    const char* what() const noexcept override { return what_.c_str(); }
};

class Reader {
public:
    Reader(std::span<const uint8_t> d, Order o) : d_(d), o_(o) {}
    size_t left() const { return d_.size() - p_; }
    size_t pos() const { return p_; }
    uint8_t u8();
    uint16_t u16();
    int16_t s16() { return (int16_t)u16(); }
    uint32_t u32();
    std::span<const uint8_t> take(size_t n);
    // What is left may only be padding (0x00, 0xA3, 0xCD).
    void done() const;

private:
    std::span<const uint8_t> d_;
    size_t p_ = 0;
    Order o_;
};

}  // namespace housefire
