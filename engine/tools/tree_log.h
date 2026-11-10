// A log of the recompiled Bustin' Out's interpreter (its src/tree_log.cpp, BUSTIN_TREE_LOG),
// read for the engine's checks (lockstep, replay).
#pragma once
#include <cstdint>
#include <cstring>
#include <fstream>
#include <array>
#include <map>
#include <memory>
#include <set>
#include <vector>

#include "housefire/bhav.h"

namespace housefire::treelog {

// ---- The log ----

struct TreeRec { uint32_t nodes; uint16_t tree; uint32_t file; std::vector<uint8_t> raw; };
// The original's expression during a step: what its variable lookup found for each side.
struct ExprRec {
    int16_t lhs_scope, lhs_data, rhs_scope, rhs_data;
    uint8_t op, flags;  // flags: 1 a 16-bit pointer on the left, 2 a float one, 4/8 a lookup failed
    int16_t rhs, lhs, lhs_after;
    uint32_t lhs_float, lhs_float_after, result;
};

// A variable lookup of the original's (any primitive's).
struct LookupRec {
    uint32_t object, caller;
    int16_t scope, data, value;
    uint8_t flags;  // 1 found, 2 a 16-bit pointer, 4 a float one
    uint32_t ptr, fptr, fvalue;
};

struct StepRec {
    uint32_t sim, frame, file, nodes;
    uint16_t tree, node, after, error;
    uint8_t look, result;
    std::vector<int16_t> params, locals, params_after, locals_after;
    int16_t defaults[8] = {};  // the interpreter's own parameters (its +44), for calls giving none
    int16_t stack_object = 0, stack_object_after = 0;  // the frame's (+4), before and after
    const TreeRec* t = nullptr;  // the tree GetNodeRef found, as loaded at the time
    bool has_expr = false;
    ExprRec expr{};
    std::vector<LookupRec> lookups;  // made during the step, in order
    uint32_t tick = 0;
    bool in_pass = false;  // a turn in the tick's pass (logs with 'Q' records)
};

// The original's object list, when it changed: each object's address and id.
struct ListRec { uint32_t tick; std::vector<uint32_t> objects; std::vector<int16_t> ids; };

// The tick's pass over the objects: the id its cursor goes on from (0: a new pass), and the id
// it stopped at (0: it went through).
struct PassRec { uint32_t from = 0, stopped_at = 0; };

// An object's place when it changed, at the start of a tick (the 'P' record): sixteenths of a
// tile, and its level; -16, -16 off the lot.
struct PlaceRec { uint32_t tick; int16_t id; int32_t x, y, level; };

// An autonomy score's call (the 'A' record): the motives it read, the adverts (16 of
// personality, minimum, delta; big-endian in the record), its score; and a base score ('B').
struct ScoreRec { uint32_t tick; float score; std::array<float, 16> motives; std::array<std::array<int16_t, 3>, 16> adverts; };
struct BaseRec { uint32_t tick; float score; };

// Autonomy's sorted list ('Y': the sort is recursive, so a tick's last is the whole list) and
// the random number its choice draws ('R').
struct ChoiceListRec { uint32_t tick; struct Entry { int16_t id; uint16_t interaction; float value; uint32_t takes_best; }; std::vector<Entry> entries; };
struct RandomRec { uint32_t tick; uint8_t site; uint32_t value; };

// A person's 16 motives when they changed, at the start of a tick (the 'N' record).
struct MotiveRec { uint32_t tick; int16_t id; std::array<float, 16> motives; };

struct Log {
    std::vector<std::unique_ptr<TreeRec>> trees;
    std::vector<ListRec> lists;
    std::map<uint32_t, PassRec> passes;  // tick -> its pass over the objects (logs with 'Q' records)
    std::vector<StepRec> steps;
    std::vector<PlaceRec> places;  // logs with 'P' records
    std::vector<MotiveRec> motives;  // logs with 'N' records
    std::vector<ScoreRec> scores;    // logs with 'A' records
    std::vector<BaseRec> bases;      // logs with 'B' records
    std::vector<ChoiceListRec> choice_lists;  // logs with 'Y' records
    std::vector<RandomRec> randoms;           // logs with 'R' records
    uint32_t ticks = 0;
};

inline bool read_log(const char* path, Log& log) {
    std::ifstream in(path, std::ios::binary);
    if (!in) return false;
    std::vector<uint8_t> b((std::istreambuf_iterator<char>(in)), {});
    size_t p = 0;
    auto u8 = [&] { return b[p++]; };
    auto u16 = [&] { uint16_t v; std::memcpy(&v, &b[p], 2); p += 2; return v; };
    auto u32 = [&] { uint32_t v; std::memcpy(&v, &b[p], 4); p += 4; return v; };
    std::map<uint32_t, const TreeRec*> at;
    ExprRec pending{};
    bool have_pending = false;
    std::vector<LookupRec> pending_lookups;
    bool in_pass = false;
    while (p < b.size()) {
        const char k = (char)b[p++];
        if (k == 'K') { if (p + 4 > b.size()) break; u32(); log.ticks++; }
        else if (k == 'L') {  // the object list, when it changed
            if (p + 6 > b.size()) break;
            ListRec l;
            l.tick = u32();
            const uint16_t cnt = u16();
            if (p + 6u * cnt > b.size()) break;
            for (int i = 0; i < cnt; i++) { l.objects.push_back(u32()); l.ids.push_back((int16_t)u16()); }
            log.lists.push_back(std::move(l));
        }
        else if (k == 'Q') {  // the pass over the objects starts: its cursor
            if (p + 4 > b.size()) break;
            log.passes[log.ticks].from = u32();
            in_pass = true;
        }
        else if (k == 'q') {  // the pass returns: 0, or the id it stopped at
            if (p + 4 > b.size()) break;
            log.passes[log.ticks].stopped_at = u32();
            in_pass = false;
        }
        else if (k == 'B') {
            if (p + 8 > b.size()) break;
            u32();
            BaseRec r{log.ticks, 0};
            const uint32_t w = u32();
            std::memcpy(&r.score, &w, 4);
            log.bases.push_back(r);
        }
        else if (k == 'Y') {  // autonomy's sorted list
            if (p + 2 > b.size()) break;
            const uint16_t cnt = u16();
            if (p + 16u * cnt > b.size()) break;
            ChoiceListRec r{log.ticks, {}};
            for (int i = 0; i < cnt; i++) {
                ChoiceListRec::Entry e{};
                e.id = (int16_t)u16();
                u16();
                e.interaction = (uint16_t)(u32() & 0xFFFF);
                const uint32_t w = u32();
                std::memcpy(&e.value, &w, 4);
                e.takes_best = u32();
                r.entries.push_back(e);
            }
            log.choice_lists.push_back(std::move(r));
        }
        else if (k == 'R') {  // the random number autonomy's choice draws
            if (p + 5 > b.size()) break;
            RandomRec r{log.ticks, u8(), 0};
            r.value = u32();
            log.randoms.push_back(r);
        }
        else if (k == 'W') {  // an evaluator's curves (tools/tree_log.py)
            if (p + 6 > b.size()) break;
            p += 4;
            const uint16_t cnt = u16();
            if (p + (size_t)cnt * (8 + 256 + 128) > b.size()) break;
            p += (size_t)cnt * (8 + 256 + 128);
        }
        else if (k == 'A') {  // an autonomy score
            if (p + 12 + 64 + 132 > b.size()) break;
            ScoreRec r{log.ticks, 0, {}, {}};
            u32(); u32();
            uint32_t w = u32();
            std::memcpy(&r.score, &w, 4);
            for (float& m : r.motives) { w = u32(); std::memcpy(&m, &w, 4); }
            const uint8_t* a = &b[p];
            for (int m = 0; m < 16; m++)
                for (int k2 = 0; k2 < 3; k2++) r.adverts[(size_t)m][(size_t)k2] = (int16_t)(a[4 + 8 * m + 2 * k2] << 8 | a[4 + 8 * m + 2 * k2 + 1]);
            p += 132;
            log.scores.push_back(r);
        }
        else if (k == 'U') {  // a call of the motive update's gate (tools/tree_log.py)
            if (p + 7 > b.size()) break;
            p += 7;
        }
        else if (k == 'N') {  // a person's motives changed (before the tick's 'K')
            if (p + 66 > b.size()) break;
            MotiveRec r{log.ticks, (int16_t)u16(), {}};
            for (float& m : r.motives) { uint32_t w = u32(); std::memcpy(&m, &w, 4); }
            log.motives.push_back(r);
        }
        else if (k == 'P') {  // an object's place changed (before the tick's 'K')
            if (p + 14 > b.size()) break;
            PlaceRec r{log.ticks, (int16_t)u16(), 0, 0, 0};
            r.x = (int32_t)u32(); r.y = (int32_t)u32(); r.level = (int32_t)u32();
            log.places.push_back(r);
        }
        else if (k == 'C') {
            if (p + 6 > b.size()) break;
            p += 6;
        }
        else if (k == 'M') {  // an object's memory (tools/tree_log.py)
            if (p + 4 + 0x1100 > b.size()) break;
            p += 4 + 0x1100;
        }
        else if (k == 'O') {  // where an object keeps its definition (tools/definitions.py)
            if (p + 4 > b.size()) break;
            p += 4;
            bool ended = false;
            while (p + 2 <= b.size()) {
                const int16_t off = (int16_t)u16();
                if (off == 0x7FFF) { ended = true; break; }
                if (p + 202 > b.size()) break;
                p += 202;
            }
            if (!ended) break;
        }
        else if (k == 'V') {
            if (p + 27 > b.size()) break;
            LookupRec v;
            v.object = u32(); v.caller = u32();
            v.scope = (int16_t)u16(); v.data = (int16_t)u16(); v.value = (int16_t)u16();
            v.flags = u8(); v.ptr = u32(); v.fptr = u32(); v.fvalue = u32();
            pending_lookups.push_back(v);
        }
        else if (k == 'E') {
            if (p + 28 > b.size()) break;
            ExprRec e;
            e.lhs_scope = (int16_t)u16(); e.lhs_data = (int16_t)u16(); e.rhs_scope = (int16_t)u16(); e.rhs_data = (int16_t)u16();
            e.op = u8(); e.flags = u8();
            e.rhs = (int16_t)u16(); e.lhs = (int16_t)u16(); e.lhs_after = (int16_t)u16();
            e.lhs_float = u32(); e.lhs_float_after = u32(); e.result = u32();
            pending = e;
            have_pending = true;
        }
        else if (k == 'T') {
            if (p + 12 > b.size()) break;
            auto t = std::make_unique<TreeRec>();
            t->nodes = u32(); t->tree = u16(); const uint16_t n = u16(); t->file = u32();
            if (p + n * 12u > b.size()) break;
            t->raw.assign(b.begin() + p, b.begin() + p + n * 12); p += n * 12;
            at[t->nodes] = t.get();
            log.trees.push_back(std::move(t));
        } else if (k == 'S') {
            if (p + 28 > b.size()) break;
            StepRec s;
            s.sim = u32(); s.frame = u32(); s.file = u32(); s.nodes = u32();
            s.tree = u16() & 0x7FFF; s.node = u16(); s.after = u16();
            s.look = u8(); s.result = u8(); s.error = u16();
            const uint8_t np = u8(), nl = u8();
            if (p + 4u * (np + nl) > b.size()) break;
            for (int i = 0; i < np; i++) s.params.push_back((int16_t)u16());
            for (int i = 0; i < nl; i++) s.locals.push_back((int16_t)u16());
            for (int i = 0; i < np; i++) s.params_after.push_back((int16_t)u16());
            for (int i = 0; i < nl; i++) s.locals_after.push_back((int16_t)u16());
            if (p + 20 > b.size()) break;
            for (int i = 0; i < 8; i++) s.defaults[i] = (int16_t)u16();
            s.stack_object = (int16_t)u16();
            s.stack_object_after = (int16_t)u16();
            auto it = at.find(s.nodes);
            if (it != at.end()) s.t = it->second;
            if (have_pending) { s.has_expr = true; s.expr = pending; have_pending = false; }
            s.tick = log.ticks;
            s.in_pass = in_pass;
            s.lookups = std::move(pending_lookups);
            pending_lookups.clear();
            log.steps.push_back(std::move(s));
        } else {
            std::printf("bad record at %zu\n", p - 1);
            return false;
        }
    }
    return true;
}

// The nodes as the original has them in memory: opcode, targets, and a call's operand (four
// 16-bit fields; a primitive's operand fields are reordered on loading by their widths,
// which are not all known, so those are not compared). Variants of an object can differ
// only in a call's operand.
inline bool same_shape(const Tree& t, const std::vector<uint8_t>& raw) {
    if (raw.size() != t.nodes.size() * 12) return false;
    for (size_t i = 0; i < t.nodes.size(); i++) {
        const uint8_t* r = raw.data() + i * 12;
        const Node& n = t.nodes[i];
        if ((uint16_t)(r[0] << 8 | r[1]) != n.opcode || r[2] != n.true_target || r[3] != n.false_target) return false;
        if ((n.opcode & 0x7FFF) > 255)
            for (int k = 0; k < 4; k++)
                if ((uint16_t)(r[4 + 2 * k] << 8 | r[5 + 2 * k]) != n.operand16(k)) return false;
    }
    return true;
}

}  // namespace housefire::treelog
