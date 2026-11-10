// Routing: where a Sim can walk (docs/simulation.md, "Routing"). Measured on the recompiled
// Bustin' Out's home log (the Sims' places, 'P').
#pragma once
#include <cstdint>
#include <functional>
#include <utility>
#include <vector>

#include "housefire/save.h"

namespace housefire {

class World;

// A lot's walls: the second array's first byte a tile (docs/lots.md, "The arrays"), a bit an
// edge of the tile: 1 towards y - 1, 2 towards x - 1, 4 towards y + 1, 8 towards x + 1 (each
// wall seen from both its tiles); 0x10 and 0x20 the diagonal walls. A door opens the wall
// between its two parts' tiles. The ground floor's (level 1) only, as the lot has one array.
class Walls {
public:
    Walls() = default;
    Walls(const std::vector<LotArray>& arrays, const World& world);
    // Whether a Sim on tile (x, y) can step to its neighbour (x + dx, y + dy), one of dx and
    // dy 0 and the other ±1. Off the grid is open (the lot's edge is not a wall here).
    bool can_step(int x, int y, int dx, int dy) const;
    // The same, the doors' openings left out.
    bool wall_between(int x, int y, int dx, int dy) const;
    // A tile split by a diagonal wall (0x10 or 0x20): closed to a Sim's steps.
    bool diagonal(int x, int y) const { return (at(x, y) & 0x30) != 0; }
    // A shortest route from tile to tile, a step at a time to a neighbouring tile (x or y by 1)
    // through open edges, round tiles `blocked` names (the goal excepted) and the diagonal
    // walls' tiles: the tiles from the start to the goal, both included; empty if none. A
    // breadth-first search, its neighbours in the order +x, -x, +y, -y.
    using Tile = std::pair<int, int>;
    std::vector<Tile> route(Tile from, Tile to, const std::function<bool(Tile)>& blocked = {}) const;
    size_t wall_edges() const { return wall_edges_; }
    size_t openings() const { return open_.size(); }

    // The rooms: the regions the walls close (a door's opening left closed), numbered from 1
    // in the order of their first tile (y, then x); 0 off the grid and on a diagonal wall's
    // tile (split between two rooms). The lot's third array is not the rooms: it marks the
    // inside (1) and the outside (7) only, the house's rooms all 1.
    uint16_t room(int x, int y) const;
    size_t rooms() const { return rooms_; }
    // A door: an opening, the tile it is left from (x + dx, y + dy beyond) and the two rooms.
    struct Door { int x, y, dx, dy; uint16_t from, to; };
    const std::vector<Door>& doors() const { return doors_; }

private:
    uint8_t at(int x, int y) const;
    uint32_t width_ = 0, height_ = 0;
    std::vector<uint8_t> edges_;
    std::vector<uint32_t> open_;  // (x, y, the edge's bit) of each door's opening
    size_t wall_edges_ = 0;
    std::vector<uint16_t> room_;
    size_t rooms_ = 0;
    std::vector<Door> doors_;
    void find_rooms();
};

}  // namespace housefire
