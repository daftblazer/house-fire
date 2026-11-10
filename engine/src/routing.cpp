#include "housefire/routing.h"

#include <algorithm>
#include <deque>

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
            if (std::find(open_.begin(), open_.end(), k) == open_.end()) {
                open_.push_back(k);
                doors_.push_back({x, y, dx, dy, 0, 0});
            }
        }
    }
    find_rooms();
}

void Walls::find_rooms() {
    room_.assign(edges_.size(), 0);
    static constexpr int kSteps[4][2] = {{1, 0}, {-1, 0}, {0, 1}, {0, -1}};
    for (uint32_t y0 = 0; y0 < height_; y0++)
        for (uint32_t x0 = 0; x0 < width_; x0++) {
            const size_t i0 = (size_t)y0 * width_ + x0;
            if (room_[i0] || diagonal((int)x0, (int)y0)) continue;
            const uint16_t r = (uint16_t)++rooms_;
            std::deque<std::pair<int, int>> q{{(int)x0, (int)y0}};
            room_[i0] = r;
            while (!q.empty()) {
                const auto [x, y] = q.front();
                q.pop_front();
                for (const auto& d : kSteps) {
                    const int nx = x + d[0], ny = y + d[1];
                    if (nx < 0 || ny < 0 || (uint32_t)nx >= width_ || (uint32_t)ny >= height_) continue;
                    const size_t n = (size_t)ny * width_ + (size_t)nx;
                    if (room_[n] || diagonal(nx, ny) || wall_between(x, y, d[0], d[1])) continue;
                    room_[n] = r;
                    q.push_back({nx, ny});
                }
            }
        }
    for (Door& d : doors_) {
        d.from = room(d.x, d.y);
        d.to = room(d.x + d.dx, d.y + d.dy);
    }
}

uint16_t Walls::room(int x, int y) const {
    if (x < 0 || y < 0 || (uint32_t)x >= width_ || (uint32_t)y >= height_ || room_.empty()) return 0;
    return room_[(size_t)y * width_ + (size_t)x];
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

std::vector<Walls::Tile> Walls::route(Tile from, Tile to, const std::function<bool(Tile)>& blocked) const {
    if (width_ == 0) return {};
    auto inside = [&](Tile t) { return t.first >= 0 && t.second >= 0 && (uint32_t)t.first < width_ && (uint32_t)t.second < height_; };
    if (!inside(from) || !inside(to)) return {};
    auto index = [&](Tile t) { return (size_t)t.second * width_ + (size_t)t.first; };
    std::vector<int32_t> prev(edges_.size(), -1);
    std::deque<Tile> q{from};
    prev[index(from)] = (int32_t)index(from);
    static constexpr int kSteps[4][2] = {{1, 0}, {-1, 0}, {0, 1}, {0, -1}};
    while (!q.empty()) {
        const Tile c = q.front();
        q.pop_front();
        if (c == to) {
            std::vector<Tile> out{c};
            for (size_t i = index(c); i != index(from);) {
                i = (size_t)prev[i];
                out.push_back({(int)(i % width_), (int)(i / width_)});
            }
            std::reverse(out.begin(), out.end());
            return out;
        }
        for (const auto& d : kSteps) {
            const Tile n{c.first + d[0], c.second + d[1]};
            if (!inside(n) || prev[index(n)] >= 0 || !can_step(c.first, c.second, d[0], d[1])) continue;
            if (n != to && (diagonal(n.first, n.second) || (blocked && blocked(n)))) continue;
            prev[index(n)] = (int32_t)index(c);
            q.push_back(n);
        }
    }
    return {};
}

}  // namespace housefire
