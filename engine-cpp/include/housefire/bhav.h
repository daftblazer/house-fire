// Behaviour trees (BHAV chunks): docs/interpreter.md, tools/chunks.py.
#pragma once
#include <array>
#include <cstdint>
#include <span>
#include <vector>

#include "housefire/iff.h"

namespace housefire {

// Node targets above 250 end the tree.
enum : uint8_t { kTargetError = 253, kReturnTrue = 254, kReturnFalse = 255 };

struct Node {
    uint16_t opcode;                  // below 256 a primitive, above a tree (tree_kind)
    uint8_t true_target, false_target;
    std::array<uint8_t, 8> operand;   // raw: its fields' order depends on the primitive
    Order order;                      // the operand's byte order (the file's)

    uint16_t operand16(int i) const;  // the operand as four 16-bit fields
};

enum class TreeKind { Primitive, Global, Own, SemiGlobal, None };
// 0-255 a primitive, 256-4095 global, 4096-8191 the object's own, 8192-9000 semi-global
// (The Sims 2's Behavior::GetTreeClass and GetResFileID).
TreeKind tree_kind(uint16_t id);

struct Tree {
    int16_t id = 0;
    uint16_t signature = 0;  // 0x8000-0x8002
    uint8_t type = 0, args = 0;
    uint16_t locals = 0;
    uint32_t version = 0;
    std::vector<Node> nodes;
};

// Throws FormatError if the chunk is not a BHAV of a known form.
Tree read_bhav(const Chunk& c, Order order);

// A file's byte order, from its first BHAV's signature (0x80 second in little-endian files).
Order bhav_order(const IffFile& f);

}  // namespace housefire
