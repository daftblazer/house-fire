#include "housefire/routing.h"

#include <algorithm>

#include "housefire/world.h"

namespace housefire {

namespace {
// The edge's bit on the tile it leaves, and on the tile it enters.
uint8_t leaving(int dx, int dy) { return dx == 1 ? 8 : dx == -1 ? 2 : dy == 1 ? 4 : dy == -1 ? 1 : 0; }
uint8_t entering(int dx, int dy) { return leaving(-dx, -dy); }
uint32_t key(int x, int y, uint8_t bit) { return (uint32_t)x << 16 | (uint32_t)y << 8 | bit; }
}  // namespace

Walls::Walls(const std::vector<LotArray>& arrays, const World& world) {
    if (arrays.size() < 2 || arrays[1].element_size < 1) return;
    const LotArray& a = arrays[1];
    width_ = a.width;
    height_ = a.height;
    edges_.resize((size_t)width_ * height_);
    // The array is row after row of y, x across a row.
    for (size_t i = 0; i < edges_.size() && i * a.element_size < a.bytes.size(); i++) edges_[i] = a.bytes[i * a.element_size];
    for (uint8_t e : edges_) wall_edges_ += __builtin_popcount(e & 0x0F);
    // The doors: each pair of a door's parts on two neighbouring tiles opens the edge between.
    for (const auto& p : world.objects()) {
        if (!p->door || !p->on_lot()) continue;
        for (const auto& q : world.objects()) {
            if (q.get() == p.get() || !q->door || q->leader != p->leader || !q->on_lot()) continue;
            const int x = p->x16 >> 4, y = p->y16 >> 4, dx = (q->x16 >> 4) - x, dy = (q->y16 >> 4) - y;
            if (std::abs(dx) + std::abs(dy) != 1) continue;
            if (dx < 0 || dy < 0) continue;  // each opening once, from its lower tile
            const uint32_t k = key(x, y, leaving(dx, dy));
            if (std::find(open_.begin(), open_.end(), k) == open_.end()) open_.push_back(k);
        }
    }
}

uint8_t Walls::at(int x, int y) const {
    if (x < 0 || y < 0 || (uint32_t)x >= width_ || (uint32_t)y >= height_) return 0;
    return edges_[(size_t)y * width_ + (size_t)x];
}

bool Walls::wall_between(int x, int y, int dx, int dy) const {
    return (at(x, y) & leaving(dx, dy)) || (at(x + dx, y + dy) & entering(dx, dy));
}

bool Walls::can_step(int x, int y, int dx, int dy) const {
    if (!wall_between(x, y, dx, dy)) return true;
    return std::find(open_.begin(), open_.end(), key(x, y, leaving(dx, dy))) != open_.end() ||
           std::find(open_.begin(), open_.end(), key(x + dx, y + dy, entering(dx, dy))) != open_.end();
}

}  // namespace housefire
