// The native interpreter against the original's, step by step: a log of the recompiled
// Bustin' Out's interpreter (its src/tree_log.cpp, BUSTIN_TREE_LOG) and the disc it ran.
//
// Usage: lockstep LOG ISO
//
// The log has every step each tree stack took. For each step the native interpreter is
// given the stack as the log has it (each frame's tree, found on the disc by the native
// lookup, its node, parameters and locals), runs one step with the primitive answering what
// the original's did (its true or false target, a yield, a stop), and its stack after is
// compared with the original's next step for the same stack:
//
//   a primitive: the node moved to the same target, or stayed (a yield);
//   a call: a frame on the same tree, found by the native lookup with the same nodes, with
//     the same parameters and locals;
//   a return: the caller's node moved to the same target.
//
// Each object's file is found from the trees the original ran for it: the disc file whose
// own, semi-global (its GLOB) and global (Global.iff) trees have those nodes.
#include <cstdio>
#include <algorithm>
#include <cstring>
#include <fstream>
#include <map>
#include <memory>
#include <climits>
#include <set>
#include <tuple>
#include <span>
#include <string>

#include "housefire/disc.h"
#include "housefire/expression.h"
#include "housefire/library.h"
#include "housefire/world.h"
#include "housefire/primitives.h"
#include "housefire/interp.h"

using namespace housefire;

namespace {

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
};

struct ListRec { uint32_t tick; std::vector<int16_t> ids; };  // the original's object list, when it changed

struct Log {
    std::vector<std::unique_ptr<TreeRec>> trees;
    std::vector<ListRec> lists;
    std::vector<StepRec> steps;
    uint32_t ticks = 0;
};

bool read_log(const char* path, Log& log) {
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
    while (p < b.size()) {
        const char k = (char)b[p++];
        if (k == 'K') { if (p + 4 > b.size()) break; u32(); log.ticks++; }
        else if (k == 'L') {  // the object list, when it changed
            if (p + 6 > b.size()) break;
            ListRec l;
            l.tick = u32();
            const uint16_t cnt = u16();
            if (p + 6u * cnt > b.size()) break;
            for (int i = 0; i < cnt; i++) { u32(); l.ids.push_back((int16_t)u16()); }
            log.lists.push_back(std::move(l));
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
bool same_shape(const Tree& t, const std::vector<uint8_t>& raw) {
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

// ---- The native side ----

// The variables as the original's lookup found them: the right side is asked for first,
// then the left (as TryExpression does); a write is kept.
struct LoggedVariables : Variables {
    const ExprRec& e;
    int asked = 0;
    bool wrote = false, wrote_float = false;
    int16_t written = 0;
    float written_float = 0;
    explicit LoggedVariables(const ExprRec& r) : e(r) {}
    std::optional<Variable> get(uint8_t, int16_t) override {
        const int i = asked++;
        if (i == 0) return (e.flags & 8) ? std::nullopt : std::optional<Variable>(Variable{e.rhs, {}});
        if (e.flags & 4) return std::nullopt;
        Variable v{e.lhs, {}};
        if ((e.flags & 3) == 2) { float f; std::memcpy(&f, &e.lhs_float, 4); v.f = f; }
        return v;
    }
    bool set(uint8_t, int16_t, int16_t v) override {
        if (!(e.flags & 1)) return false;  // the original's left side has no 16-bit pointer
        wrote = true;
        written = v;
        return true;
    }
    bool set_float(uint8_t, int16_t, float v) override {
        if (!(e.flags & 2)) return false;
        wrote_float = true;
        written_float = v;
        return true;
    }
};

// Answers the one primitive of the step as the original did.
struct LogHost : Host {
    const Library* lib = nullptr;
    const ObjectFile* object = nullptr;
    PrimResult answer = PrimResult::Stop;
    std::vector<int16_t> defaults;
    PrimResult primitive(Interpreter&, Frame&, const Node&) override { return answer; }
    std::span<const int16_t> default_params() override { return defaults; }
    const Tree* tree(uint16_t id) override { return lib->tree(object, id); }
};

struct FrameState {
    uint16_t tree, node;
    std::vector<int16_t> params, locals;
    const ObjectFile* object;  // each frame its own: a Sim runs an object's interaction on its stack
};

}  // namespace

int main(int argc, char** argv) {
    if (argc < 3) { std::printf("usage: lockstep LOG ISO\n"); return 2; }
    Log log;
    if (!read_log(argv[1], log)) { std::printf("cannot read %s\n", argv[1]); return 1; }
    Disc d(argv[2]);
    if (!d.ok() || d.id() != "G4ME69") { std::printf("%s is not Bustin' Out\n", argv[2]); return 1; }

    const Library lib = Library::bustin_out(d);
    const auto& all = lib.files();
    // Each object's file: the disc file under which every tree the original ran for it is
    // found with the same nodes. Memory is reused, so a file pointer's trees are taken in
    // order and split where no one disc file has them all (another object at that address);
    // each part is an object.
    auto fits = [&](const ObjectFile* f, const TreeRec* t) {
        const Tree* nt = lib.tree(f, t->tree);
        return nt && same_shape(*nt, t->raw);
    };
    // A constant (scope 26), as the library finds it; `where` says where it looked.
    auto constant = [&](const ObjectFile* object, int16_t data, std::string* where = nullptr) {
        if (where) {
            char w[96];
            std::snprintf(w, sizeof w, "datum %#x in %s", (uint16_t)data, object ? object->path.c_str() : "-");
            *where = w;
        }
        return lib.constant(object, data);
    };
    // The constants a step's expression read, as (datum, value).
    auto constants_read = [](const StepRec& s) {
        std::vector<std::pair<int16_t, int16_t>> out;
        if (!s.has_expr) return out;
        if (s.expr.rhs_scope == 26 && !(s.expr.flags & 8)) out.push_back({s.expr.rhs_data, s.expr.rhs});
        if (s.expr.lhs_scope == 26 && !(s.expr.flags & 4)) out.push_back({s.expr.lhs_data, s.expr.lhs});
        return out;
    };

    // The sightings are the steps: each names its object's file pointer and the tree run (a
    // tree is logged once, under the first object that ran it, but every step has its own).
    struct Part { std::vector<const ObjectFile*> files; };
    std::vector<Part> parts;
    std::map<uint32_t, size_t> part_now;                 // file pointer -> its current part
    std::vector<long> step_part(log.steps.size(), -1);
    for (size_t i = 0; i < log.steps.size(); i++) {
        const StepRec& s = log.steps[i];
        auto it = part_now.find(s.file);
        if (!s.t || tree_kind(s.t->tree) == TreeKind::Global || tree_kind(s.t->tree) == TreeKind::None) {
            if (it != part_now.end()) step_part[i] = (long)it->second;
            continue;
        }
        if (it != part_now.end()) {
            std::vector<const ObjectFile*> keep;
            for (const ObjectFile* f : parts[it->second].files) if (fits(f, s.t)) keep.push_back(f);
            if (!keep.empty()) {
                parts[it->second].files = std::move(keep);
                step_part[i] = (long)it->second;
                continue;
            }
        }
        Part p;
        for (const auto& f : all) if (fits(f.get(), s.t)) p.files.push_back(f.get());
        part_now[s.file] = parts.size();
        step_part[i] = (long)parts.size();
        parts.push_back(std::move(p));
    }
    // Variants of an object share their trees but not always their constants: the constants
    // each part's steps read narrow it further (where some file has them all).
    for (size_t i = 0; i < log.steps.size(); i++) {
        if (step_part[i] < 0) continue;
        Part& p = parts[step_part[i]];
        for (auto [data, value] : constants_read(log.steps[i])) {
            if ((uint16_t)data >> 7 >= 64) continue;  // only the object's own tables tell variants apart
            std::vector<const ObjectFile*> keep;
            for (const ObjectFile* f : p.files) if (constant(f, data) == value) keep.push_back(f);
            if (!keep.empty()) p.files = std::move(keep);
        }
    }
    std::vector<const ObjectFile*> step_object(log.steps.size(), nullptr);
    for (size_t i = 0; i < log.steps.size(); i++)
        if (step_part[i] >= 0 && !parts[step_part[i]].files.empty()) step_object[i] = parts[step_part[i]].files[0];
    size_t found_parts = 0;
    for (const Part& p : parts) found_parts += !p.files.empty();
    const int unresolved = (int)(parts.size() - found_parts);
    std::printf("%u ticks, %zu steps, %zu trees loaded; objects %zu, found on the disc %zu\n",
                log.ticks, log.steps.size(), log.trees.size(), parts.size(), found_parts);

    // The steps.
    std::map<std::string, long> out;
    std::map<std::string, std::string> example;
    auto note = [&](const std::string& k, const std::string& ex = {}) {
        out[k]++;
        if (!ex.empty() && !example.count(k)) example[k] = ex;
    };
    // The stack's memory as far as the log shows it: each frame's values where they are
    // (+16 on), as last seen. For telling whether a new frame's values are left over.
    std::map<uint32_t, int16_t> memory;
    auto remember = [&](uint32_t frame, const std::vector<int16_t>& p, const std::vector<int16_t>& l) {
        for (size_t k = 0; k < p.size(); k++) memory[frame + 16 + 2 * (uint32_t)k] = p[k];
        for (size_t k = 0; k < l.size(); k++) memory[frame + 16 + 2 * (uint32_t)(p.size() + k)] = l[k];
    };
    std::map<uint32_t, std::vector<uint32_t>> stacks;          // sim -> frame pointers, bottom first
    std::map<uint32_t, FrameState> frames;                      // frame pointer -> state
    std::map<uint32_t, size_t> next_of;                         // index -> next step of its sim
    {
        std::map<uint32_t, size_t> last;
        for (size_t i = 0; i < log.steps.size(); i++) {
            auto it = last.find(log.steps[i].sim);
            if (it != last.end()) next_of[(uint32_t)it->second] = i;
            last[log.steps[i].sim] = i;
        }
    }
    for (size_t i = 0; i < log.steps.size(); i++) {
        const StepRec& s = log.steps[i];
        auto& st = stacks[s.sim];
        // Frames are end to end, bottom first: a frame at or below the top replaces what was
        // above it (the engine also pops and resets stacks outside the node step).
        while (!st.empty() && st.back() > s.frame) st.pop_back();
        if (st.empty() || st.back() != s.frame) st.push_back(s.frame);
        frames[s.frame] = {s.tree, s.node, s.params, s.locals, step_object[i]};
        struct After {
            decltype(remember)& r; std::map<uint32_t, int16_t>& m; const StepRec& s;
            ~After() { r(s.frame, s.params_after, s.locals_after); if (s.after <= 255) m[s.frame + 2] = (int16_t)s.after; }
        } after{remember, memory, s};
        remember(s.frame, s.params, s.locals);
        // The header's words the log has: +0 the tree, +2 the node, +6 the counts, +12 the file.
        memory[s.frame] = (int16_t)s.tree;  // (its flag bit is not kept)
        memory[s.frame + 2] = (int16_t)s.node;
        memory[s.frame + 6] = (int16_t)(s.locals.size() << 8 | s.params.size());
        memory[s.frame + 12] = (int16_t)(s.file >> 16);
        memory[s.frame + 14] = (int16_t)s.file;

        if (!step_object[i]) { note("object not found"); continue; }
        if (s.look == 0) { note("step without a node lookup (frame end at the bottom, or no file)"); continue; }

        // The native stack, as the log has it.
        LogHost h;
        h.lib = &lib;
        h.object = step_object[i];
        h.defaults.assign(s.defaults, s.defaults + 8);
        std::vector<Frame> native;
        bool ok = true;
        for (uint32_t fp : st) {
            const FrameState& fs = frames[fp];
            Frame f;
            f.tree = fs.object ? lib.tree(fs.object, fs.tree) : nullptr;
            if (!f.tree) { ok = false; break; }
            f.node = fs.node;
            f.params = fs.params;
            f.locals = fs.locals;
            native.push_back(std::move(f));
        }
        if (!ok) {
            std::string e = "object " + step_object[i]->path + ", stack:";
            for (uint32_t fp : st) { char b[32]; std::snprintf(b, sizeof b, " %#x@%x", frames[fp].tree, fp); e += b; }
            note("a frame's tree not found by the native lookup", e);
            continue;
        }

        const Frame& top = native.back();
        const bool is_return = s.node > 250;
        const Node* n = is_return || s.node >= top.tree->nodes.size() ? nullptr : &top.tree->nodes[s.node];
        if (n && n->opcode < 256) {
            if (s.result == 1) h.answer = PrimResult::Yield;
            else if (s.result == 2) h.answer = PrimResult::Stop;
            else h.answer = s.after == n->true_target ? PrimResult::True : PrimResult::False;
        }
        Interpreter in(h, native.size() == 1 ? Interpreter::Mode::Check : Interpreter::Mode::Main);
        in.restore(native);
        const Step step = in.step();

        auto nx = next_of.find((uint32_t)i);
        const StepRec* next = nx == next_of.end() ? nullptr : &log.steps[nx->second];
        char ex[256];
        if (is_return) {
            if (native.size() == 1) { note("return from the bottom (a check tree's end)"); continue; }
            const uint32_t caller = st[st.size() - 2];
            if (!next) { note("no next step"); continue; }
            std::snprintf(ex, sizeof ex, "tree %#x@%x returns %u; caller %#x@%x node %u: native %u, original %#x@%x node %u; depth %zu",
                          s.tree, s.frame, s.node, frames[caller].tree, caller, frames[caller].node,
                          in.stack().empty() ? 0 : in.stack().back().node, next->tree, next->frame, next->node, st.size());
            if (next->frame == caller && in.stack().size() == native.size() - 1 && in.stack().back().node == next->node)
                note("return: same");
            else note("return: DIFFERENT", ex);
        } else if (n && n->opcode >= 256) {
            if (!next) { note("no next step"); continue; }
            const Frame& callee = in.stack().back();
            std::snprintf(ex, sizeof ex, "tree %#x node %u calls %#x: params native %zu, original %zu",
                          s.tree, s.node, n->opcode, callee.params.size(), next->params.size());
            if (in.stack().size() == native.size() + 1 && next->frame > s.frame && next->node == 0 &&
                next->tree != n->opcode && next->look != 'R') {
                // The callee's frame was replaced before it ran, from outside the node step
                // (the stack reset for a new action).
                note("call: callee replaced from outside before its first step");
                continue;
            }
            if (in.stack().size() != native.size() + 1 || next->frame == s.frame || next->tree != n->opcode) {
                note("call: DIFFERENT (no callee frame)", ex);
                continue;
            }
            note(next->t && same_shape(*callee.tree, next->t->raw) ? "call: callee tree the same" : "call: callee tree DIFFERENT", ex);
            auto vals = [](const std::vector<int16_t>& v) {
                std::string o = "(";
                for (int16_t x : v) o += std::to_string(x) + " ";
                return o + ")";
            };
            char op[64];
            std::snprintf(op, sizeof op, " operand %04x %04x %04x %04x;", n->operand16(0), n->operand16(1), n->operand16(2), n->operand16(3));
            const std::string ev = std::string(ex) + op + " caller params " + vals(native.back().params) + " locals " + vals(native.back().locals) +
                "; callee params native " + vals(callee.params) + " original " + vals(next->params) +
                ", locals native " + vals(callee.locals) + " original " + vals(next->locals);
            note(callee.params == next->params ? "call: parameters the same" : "call: parameters DIFFERENT", ev);
            // A new frame's locals are zero here and left-over memory in the original: by
            // design (docs/interpreter.md, "The call"), so counted but not a difference.
            const bool zero = std::all_of(next->locals.begin(), next->locals.end(), [](int16_t v) { return v == 0; });
            note(callee.locals == next->locals ? "call: locals the same"
                 : zero ? "call: locals DIFFERENT" : "call: locals left over in the original (zero here, by design)", ev);
            // Are the original's values what the stack's memory held there before?
            auto left_over = [&](size_t first, const std::vector<int16_t>& v) {
                int known = 0, same = 0;
                for (size_t k = 0; k < v.size(); k++) {
                    auto m = memory.find(next->frame + 16 + 2 * (uint32_t)(first + k));
                    if (m == memory.end()) continue;
                    known++;
                    same += m->second == v[k];
                }
                return known == 0 ? "unknown" : same == known ? "left over" : "not left over";
            };
            const bool none = std::all_of(n->operand.begin(), n->operand.end(), [](uint8_t b) { return b == 0xFF; });
            if (none && callee.params != next->params) {
                std::string where = left_over(0, next->params);
                // Or some frame's parameters further down this stack?
                for (size_t k = native.size(); k-- > 0;) {
                    const auto& pp = native[k].params;
                    if (!pp.empty() && std::equal(next->params.begin(), next->params.begin() + std::min(pp.size(), next->params.size()), pp.begin())) {
                        where = "a frame " + std::to_string(native.size() - k) + " down";
                        break;
                    }
                }
                std::string stack_vals;
                for (const Frame& f : native) stack_vals += " " + std::to_string(f.tree->id) + vals(f.params) + vals(f.locals);
                note(std::string("  parameters (none given, differ): ") + where, ev + "; stack" + stack_vals);
            }
            if (callee.locals != next->locals)
                note(std::string("  locals left over: ") + left_over(next->params.size(), next->locals), ev);
        } else if (n) {
            if (n->opcode == 2) {
                // The native expression on the values the original read.
                if (!s.has_expr) note("expression: no record");
                else {
                    const ExprRec& e = s.expr;
                    LoggedVariables vars(e);
                    const ExpressionOperand op = ExpressionOperand::of(*n);
                    for (auto [data, value] : constants_read(s)) {
                        std::string where;
                        const std::optional<int16_t> found = constant(step_object[i], data, &where);
                        where += ": native " + (found ? std::to_string(*found) : std::string("none")) + ", original " + std::to_string(value);
                        note(!found ? "constant: not found by the native lookup" : *found == value ? "constant: same" : "constant: DIFFERENT", where);
                    }
                    const ExprResult er = expression(op, vars);
                    const PrimResult r = er.result;
                    char d[220];
                    std::snprintf(d, sizeof d, "op %u lhs %d:%d=%d rhs %d:%d=%d flags %u: original result %d after %d, native result %d error %d wrote %d(%d)",
                                  e.op, e.lhs_scope, e.lhs_data, e.lhs, e.rhs_scope, e.rhs_data, e.rhs, e.flags, (int)h.answer,
                                  e.lhs_after, (int)r, (int)er.error, vars.wrote, vars.written);
                    const std::string key = "expression op " + std::to_string(e.op);
                    const bool either = n->true_target == n->false_target;
                    if (op.op != e.op || op.lhs_scope != (uint8_t)e.lhs_scope || op.rhs_scope != (uint8_t)e.rhs_scope ||
                        op.lhs_data != e.lhs_data || op.rhs_data != e.rhs_data)
                        note("expression: operand read differently", d);
                    else {
                        // The result, unless both targets are the same node (then the step
                        // does not show it), and the value written.
                        const bool same_result = either || r == h.answer;
                        bool same_value;
                        if ((e.flags & 3) == 2) {
                            uint32_t bits = e.lhs_float;
                            if (vars.wrote_float) std::memcpy(&bits, &vars.written_float, 4);
                            same_value = bits == e.lhs_float_after;
                        } else
                            same_value = !(e.flags & 1) || (vars.wrote ? vars.written == e.lhs_after : e.lhs == e.lhs_after);
                        const std::string kind = (e.flags & 3) == 2 ? " (float)" : "";
                        note(key + kind + (same_result && same_value ? ": same" : same_result ? ": value DIFFERENT" : ": result DIFFERENT"), d);
                    }
                }
            }
            const char* what = s.result == 1 ? "yield" : s.result == 2 ? "stop" : "transition";
            std::snprintf(ex, sizeof ex, "tree %#x node %u op %u result %u: native node %u, original %u",
                          s.tree, s.node, n->opcode, s.result, in.stack().empty() ? 0 : in.stack().back().node, s.after);
            const bool same = s.result == 2 ? step == Step::Stop
                            : !in.stack().empty() && in.stack().back().node == s.after &&
                              (s.result != 1 || step == Step::Yield);
            note(std::string("primitive ") + what + (same ? ": same" : ": DIFFERENT"), ex);
        } else {
            note("node not in the tree", std::to_string(s.node));
        }
    }
    // ---- The variables: the native object model against the original's lookups ----
    //
    // A native object for each object the original asks for or names as a stack object.
    // Each variable cell takes its value from the log the first time it is seen; after that
    // the native value must be what the log reads, unless something other than a script's
    // expression wrote it (the engine, or a primitive not modelled yet: counted apart). And
    // each cell must always be the same address in the original, and each address the same
    // cell: that checks what the scopes name.
    {
        World world(lib);
        std::map<uint32_t, Object*> native;     // the original's object pointer -> native object
        std::map<std::pair<const void*, int>, uint32_t> cell_addr;
        std::map<uint32_t, std::pair<const void*, int>> addr_cell;
        std::map<std::pair<const void*, int>, bool> seen;
        std::map<uint32_t, uint32_t> last_writer;  // address -> the caller that last had its pointer
        std::map<std::pair<const void*, int>, const Object*> cell_owner;
        std::map<uint32_t, size_t> addr_used;        // address -> the last step that reached it
        std::map<const Object*, size_t> last_used;   // native object -> the last step that used it
        // The object whose storage a lookup reached (frame and shared tables: none).
        auto owner_object = [&](const LookupRec& v, Object& me, const Frame& f) -> const Object* {
            switch (v.scope) {
                case 0: case 3: case 8: case 14: case 18: case 30: return &me;
                case 1: case 4: case 15: case 19: case 22: return world.find(f.stack_object);
                default: return nullptr;
            }
        };
        // Forget an object that is gone: its native object, its cells and their addresses.
        auto drop = [&](const Object* o) {
            for (auto it = cell_owner.begin(); it != cell_owner.end();) {
                if (it->second == o) {
                    auto ca = cell_addr.find(it->first);
                    if (ca != cell_addr.end()) { addr_cell.erase(ca->second); cell_addr.erase(ca); }
                    seen.erase(it->first);
                    it = cell_owner.erase(it);
                } else ++it;
            }
            for (auto it = native.begin(); it != native.end();) it = it->second == o ? native.erase(it) : std::next(it);
            world.remove(o);
        };
        const uint32_t kExprLhs = 0x80213900, kExprRhs = 0x802138C8;
        std::map<std::string, std::map<std::string, long>> why;  // a difference -> (tick band, caller) counts
        uint32_t loaded_at = UINT32_MAX;  // the tick the current lot started loading
        auto differ = [&](const std::string& k0, const StepRec& s, const LookupRec& v) {
            // While a lot loads the engine sets its objects up itself (their ids among them):
            // counted apart.
            const bool loading = loaded_at != UINT32_MAX && s.tick >= loaded_at && s.tick < loaded_at + 25;
            const std::string k = loading ? k0.substr(0, k0.find(" DIFFERENT")) + " (while a lot loads)" : k0;
            note(k);
            char b[64];
            std::snprintf(b, sizeof b, "tick %u-%u caller %x", s.tick / 20 * 20, s.tick / 20 * 20 + 19, v.caller);
            why[k][b]++;
        };
        auto object_at = [&](uint32_t ptr, const ObjectFile* file) -> Object& {
            auto it = native.find(ptr);
            if (it != native.end()) return *it->second;
            auto o = std::make_unique<Object>();
            o->file = file;
            Object& r = world.add(std::move(o));
            native[ptr] = &r;
            return r;
        };
        // Where a stack-object scope's variable is in the original: back from its address to
        // the object's (the layouts in docs/objects.md).
        auto owner_of = [](const LookupRec& v) -> std::optional<uint32_t> {
            const uint32_t a = v.ptr ? v.ptr : v.fptr;
            if (!a) return std::nullopt;  // the value alone was asked for
            switch (v.scope) {
                case 4: return a - 0x26 - 2 * (uint32_t)v.data;
                case 15: return a + 0x598 - 4 * (uint32_t)v.data;
                case 19: return a + 0x638 - 2 * (uint32_t)v.data;
                default: return std::nullopt;
            }
        };
        // A lot loaded: the ticks at which 20 or more objects never seen before ask for
        // variables (the dream at 0, the club at 2799 in the new-game route).
        std::set<uint32_t> lot_ticks;
        {
            std::set<uint32_t> seen_objects;
            std::map<uint32_t, int> fresh;
            for (const StepRec& s : log.steps)
                for (const LookupRec& v : s.lookups)
                    if (seen_objects.insert(v.object).second) fresh[s.tick]++;
            for (const auto& [t, n] : fresh) if (n >= 20) lot_ticks.insert(t);
        }
        uint32_t lot_tick = UINT32_MAX;
        for (size_t i = 0; i < log.steps.size(); i++) {
            const StepRec& s = log.steps[i];
            if (s.lookups.empty()) continue;
            // (a load's objects may come over two ticks: one lot)
            if (lot_ticks.count(s.tick) && (lot_tick == UINT32_MAX || s.tick > lot_tick + 1)) {
                if (lot_tick != UINT32_MAX) note("variables: a lot loaded (the native world cleared)");
                world.clear();
                native.clear(); cell_addr.clear(); addr_cell.clear(); seen.clear(); last_writer.clear();
                cell_owner.clear(); last_used.clear(); addr_used.clear();
            }
            if (lot_ticks.count(s.tick)) {
                if (lot_tick == UINT32_MAX || s.tick > lot_tick + 1) loaded_at = s.tick;
                lot_tick = s.tick;
            }
            Frame frame;
            frame.params = s.params;
            frame.locals = s.locals;
            frame.stack_object = s.stack_object;
            frame.file = step_object[i];
            for (const LookupRec& v : s.lookups) {
                Object& me = object_at(v.object, step_object[i]);
                me.person = me.person || v.scope == 14 || v.scope == 18;
                last_used[&me] = i;
                if (Object* so = world.find(s.stack_object)) last_used[so] = i;
                ScriptVariables vars(world, me, frame);
                const std::string sc = "scope " + std::to_string(v.scope);
                if (!(v.flags & 1)) { note("variables: " + sc + ": the original's lookup failed"); continue; }
                if (v.scope == 7 || v.scope == 26) {
                    if (v.caller != kExprLhs && v.caller != kExprRhs) continue;
                    const auto g = vars.get((uint8_t)v.scope, v.data);
                    if (g && g->value == v.value) note("variables: " + sc + ": same"); else differ("variables: " + sc + ": DIFFERENT", s, v);
                    continue;
                }
                // A stack object the native world does not have yet: made from the original's.
                if (auto q = owner_of(v)) {
                    Object* so = world.find(s.stack_object);
                    if (!so) {
                        Object& n = object_at(*q, nullptr);
                        if (n.data.size() <= kDataObjectId) n.data.resize(kDataObjectId + 1);
                        n.data[kDataObjectId] = s.stack_object;
                        world.reindex(n);
                        so = &n;
                    }
                    so->person = so->person || v.scope == 15 || v.scope == 19;
                    auto bound = native.find(*q);
                    if (bound != native.end() && bound->second == so) note("variables: stack object the same");
                    else {
                        differ("variables: stack object DIFFERENT", s, v);
                        static int shown = 0;
                        if (shown++ < 6)
                            std::printf("  stack object %d: by id native %p (id %d), at the original's address %x native %p (id %d); scope %d caller %x\n",
                                        s.stack_object, (void*)so, so->id(), *q, bound == native.end() ? nullptr : (void*)bound->second,
                                        bound == native.end() ? -1 : bound->second->id(), v.scope, v.caller);
                    }
                    if (bound == native.end()) native[*q] = so;
                }
                const auto c = vars.cell((uint8_t)v.scope, v.data);
                if (!c) { note("variables: " + sc + ": not modelled yet"); continue; }
                const uint32_t addr = v.ptr ? v.ptr : v.fptr;
                const std::pair<const void*, int> key{c->table, c->index};
                // Frame variables are the step's own: rebuilt from the log each step.
                const bool in_frame = v.scope == 9 || v.scope == 25 || v.scope == 10;
                if (addr && !in_frame) {
                    auto ca = cell_addr.find(key);
                    auto ac = addr_cell.find(addr);
                    // A binding unused for a long time is stale: the memory was reused, or the
                    // object at a pointer replaced (another lot loaded). Forget it.
                    auto stale = [&](uint32_t a) { auto u = addr_used.find(a); return u == addr_used.end() || i - u->second > 20000; };
                    if (ac != addr_cell.end() && ac->second != key && stale(addr)) {
                        cell_addr.erase(ac->second); seen.erase(ac->second); addr_cell.erase(ac);
                        note("variables: memory reused");
                    }
                    ca = cell_addr.find(key);
                    if (ca != cell_addr.end() && ca->second != addr && stale(ca->second)) {
                        addr_cell.erase(ca->second); seen.erase(key); cell_addr.erase(ca);
                        note("variables: object replaced");
                    }
                    ca = cell_addr.find(key);
                    ac = addr_cell.find(addr);
                    addr_used[addr] = i;
                    if (ca == cell_addr.end() && ac == addr_cell.end()) { cell_addr[key] = addr; addr_cell[addr] = key; cell_owner[key] = owner_object(v, me, frame); }
                    else if ((ca != cell_addr.end() && ca->second != addr) || (ac != addr_cell.end() && ac->second != key))
                        differ("variables: " + sc + ": where DIFFERENT", s, v);
                    else note("variables: " + sc + ": where the same");
                }
                if (in_frame) {
                    if (v.caller != kExprLhs && v.caller != kExprRhs) continue;  // the address only
                    const auto g = vars.get((uint8_t)v.scope, v.data);
                    if (g && g->value == v.value) note("variables: " + sc + ": value the same"); else differ("variables: " + sc + ": value DIFFERENT", s, v);
                    continue;
                }
                // The value: seeded once, then the native one must hold. Only the
                // expression asks for the value; other primitives take the address alone (the
                // log's value is then not one).
                if (v.caller != kExprLhs && v.caller != kExprRhs) {
                    if (addr && (v.flags & 6)) last_writer[addr] = v.caller;
                    seen[key] = false;  // it may write there: take the next value read as the new one
                    continue;
                }
                const bool is_float = (v.flags & 4) && !(v.flags & 2);
                auto get = vars.get((uint8_t)v.scope, v.data);
                if (!seen[key]) {
                    if (is_float) { float f; std::memcpy(&f, &v.fvalue, 4); vars.set_float((uint8_t)v.scope, v.data, f); }
                    else vars.set((uint8_t)v.scope, v.data, v.value);
                    seen[key] = true;
                    note("variables: " + sc + ": first seen");
                } else {
                    bool same;
                    if (is_float) { uint32_t bits; float f = get && get->f ? *get->f : 0; std::memcpy(&bits, &f, 4); same = bits == v.fvalue; }
                    else same = get && get->value == v.value;
                    if (same) note("variables: " + sc + ": value the same");
                    else {
                        const auto w = last_writer.find(addr);
                        const bool other = w != last_writer.end() && w->second != kExprLhs && w->second != kExprRhs;
                        note("variables: " + sc + (other ? ": changed by a primitive not modelled yet" : ": changed by the engine"));
                        if (is_float) { float f; std::memcpy(&f, &v.fvalue, 4); vars.set_float((uint8_t)v.scope, v.data, f); }
                        else vars.set((uint8_t)v.scope, v.data, v.value);
                    }
                }
                if (addr && (v.flags & 6)) last_writer[addr] = v.caller;
            }
            // The expression's write: the native cell takes the value after (the expression
            // itself is checked above).
            if (s.has_expr && !s.lookups.empty() && s.lookups.back().caller == kExprLhs) {
                const LookupRec& v = s.lookups.back();
                Object& me = object_at(v.object, step_object[i]);
                ScriptVariables vars(world, me, frame);
                if (s.expr.flags & 1) vars.set((uint8_t)v.scope, v.data, s.expr.lhs_after);
                else if (s.expr.flags & 2) { float f; std::memcpy(&f, &s.expr.lhs_float_after, 4); vars.set_float((uint8_t)v.scope, v.data, f); }
            }
        }
        for (const auto& [k, m] : why) {
            std::printf("%s:\n", k.c_str());
            std::vector<std::pair<long, std::string>> top;
            for (const auto& [w, n] : m) top.push_back({n, w});
            std::sort(top.rbegin(), top.rend());
            for (size_t j = 0; j < top.size() && j < 4; j++) std::printf("    %6ld  %s\n", top[j].first, top[j].second.c_str());
        }
    }

    // ---- SetToNext on a world loaded from the lot ----
    //
    // For each lot the log shows (a new lot: 20 or more objects never seen before ask for
    // variables in one tick), the disc's lot whose objects best match the ids seen then,
    // loaded natively; the objects that appear at the load and are not in the lot's table
    // (two in the club) go in front, not Sims. Each SetToNext step the original took on the
    // stack object is run natively on that world, from the step's frame.
    {
        const auto lots = read_lots(d.read("Runtime/default.ngh"));
        std::vector<std::set<int16_t>> lot_ids;
        for (const LotChunks& l : lots) {
            std::set<int16_t> ids;
            for (const LotObject& o : read_lot_objects(*l.find("ObjM")).objects) ids.insert(o.id);
            lot_ids.push_back(std::move(ids));
        }
        // Which lot: the one whose table order the original's longest walk over every object
        // (SetToNext kind 0 from 0 on the stack object) follows furthest. Object ids are
        // small numbers every lot reuses, so they do not tell lots apart; the order does.
        std::vector<int16_t> walk, cur;
        bool walking = false;
        for (const StepRec& s : log.steps) {
            if (s.look != 'X' || !s.t || s.t->tree != s.tree || s.node * 12u >= s.t->raw.size()) continue;
            const uint8_t* raw = s.t->raw.data() + 12 * s.node;
            if (((raw[0] << 8 | raw[1]) & 0x7FFF) != 31 || (raw[8] & 0x7F) != 0 || raw[9] != 0x0A || raw[2] == raw[3]) continue;
            if (s.stack_object == 0) { cur.clear(); walking = true; }
            if (!walking) continue;
            if (s.after == raw[2]) { cur.push_back(s.stack_object_after); if (cur.size() > walk.size()) walk = cur; }
            else walking = false;
        }
        size_t best = 0, best_n = 0;
        std::vector<std::vector<int16_t>> orders;
        for (const LotChunks& l : lots) {
            std::vector<int16_t> o;
            for (const LotObject& x : read_lot_objects(*l.find("ObjM")).objects) o.push_back(x.id);
            orders.push_back(std::move(o));
        }
        for (size_t k = 0; k < lots.size(); k++) {
            size_t n = 0;
            for (int16_t id : walk) {
                if (!lot_ids[k].count(id)) continue;
                if (n < orders[k].size() && orders[k][n] == id) n++; else break;
            }
            if (n > best_n) { best = k; best_n = n; }
        }
        {
            const uint32_t from = 0, to = UINT32_MAX;
            const std::set<int16_t> seen(walk.begin(), walk.end());
            World lw(lib);
            lw.load_lot(lots[best]);
            // Objects made at load: in the walk but not in the lot, in front in the walk's order
            // (when the log has the original's list, it is replayed instead, below).
            std::vector<int16_t> made;
            if (log.lists.empty())
                for (int16_t id : walk) if (!lot_ids[best].count(id)) made.push_back(id);
            size_t li = 0;
            for (auto it = made.rbegin(); it != made.rend(); ++it) {
                auto o = std::make_unique<Object>();
                o->data.resize(kDataObjectId + 1);
                o->data[kDataObjectId] = *it;
                lw.add(std::move(o), true);
            }
            char lb[96];
            std::snprintf(lb, sizeof lb, "SetToNext: the disc's lot %zu (the walk of %zu follows its order for %zu)", best, walk.size(), best_n);
            note(lb);
            // The lot's saved state against the original: the first read of each of an
            // object's own variables (data, attributes, temps), before any expression wrote
            // it, must be the lot's value. Objects by their id (data 11, read as they ask).
            {
                std::map<uint32_t, int16_t> id_of;  // the original's object pointer -> id
                std::set<std::tuple<uint32_t, int, int>> done;
                for (const StepRec& s : log.steps) {
                    for (const LookupRec& v : s.lookups) {
                        if (v.caller != 0x80213900 && v.caller != 0x802138C8) continue;
                        if (v.scope == 3 && v.data == kDataObjectId) id_of[v.object] = v.value;
                        if (v.scope != 3 && v.scope != 0 && v.scope != 8) continue;
                        const auto key = std::make_tuple(v.object, (int)v.scope, (int)v.data);
                        if (!done.insert(key).second) continue;
                        auto id = id_of.find(v.object);
                        if (id == id_of.end()) { note("lot state: object not known yet"); continue; }
                        const Object* o = lw.find(id->second);
                        if (!o) { note("lot state: object not in the lot"); continue; }
                        const std::vector<int16_t>* t = v.scope == 3 ? &o->data : v.scope == 0 ? &o->attributes : nullptr;
                        int16_t native;
                        if (t) {
                            if (v.data < 0 || (size_t)v.data >= t->size()) { note("lot state: scope " + std::to_string(v.scope) + ": past the table"); continue; }
                            native = (*t)[(size_t)v.data];
                        } else {
                            if (v.data < 0 || v.data >= 8) continue;
                            native = o->temps[(size_t)v.data];
                        }
                        note("lot state: scope " + std::to_string(v.scope) + (native == v.value ? ": the first read the lot's value" : ": the first read not the lot's (changed before it)"));
                    }
                    // A written variable's first read after it is not the lot's to check.
                    if (s.has_expr && !s.lookups.empty() && s.lookups.back().caller == 0x80213900)
                        done.insert(std::make_tuple(s.lookups.back().object, (int)s.lookups.back().scope, (int)s.lookups.back().data));
                }
            }
            Object me;
            for (size_t i = 0; i < log.steps.size(); i++) {
                const StepRec& s = log.steps[i];
                if (s.tick < from || s.tick >= to || s.look != 'X' || !s.t || s.t->tree != s.tree) continue;
                const uint8_t* raw = s.t->raw.data() + 12 * s.node;
                if (((raw[0] << 8 | raw[1]) & 0x7FFF) != 31 || s.node * 12u >= s.t->raw.size()) continue;
                if (raw[2] == raw[3]) continue;
                // The disc's node (its operand in the file's order).
                const Tree* tree = step_object[i] ? lib.tree(step_object[i], s.tree) : nullptr;
                if (!tree || s.node >= tree->nodes.size()) continue;
                const Node& node = tree->nodes[s.node];
                if ((node.operand[4] & 0x80) && node.operand[5] != (uint8_t)Scope::StackObject) continue;
                Frame f;
                f.params = s.params;
                f.locals = s.locals;
                f.stack_object = s.stack_object;
                ScriptVariables vars(lw, me, f);
                // The objects made and removed, as the original's list shows them, replayed up to
            // this step's tick (each new id checked against the world's rule).
            while (li < log.lists.size() && log.lists[li].tick <= s.tick) {
                const ListRec& L = log.lists[li++];
                std::set<int16_t> now(L.ids.begin(), L.ids.end());
                std::vector<const Object*> gone;
                for (const auto& o : lw.objects()) if (!now.count(o->id())) gone.push_back(o.get());
                for (const Object* o : gone) { lw.remove(o); note("objects: removed"); }
                for (auto it = L.ids.rbegin(); it != L.ids.rend(); ++it) {
                    if (lw.find(*it)) continue;
                    const int16_t want = lw.next_id();
                    lw.make_object(std::make_unique<Object>());
                    note(want == *it ? "objects: made, the id the same" : "objects: made, the id DIFFERENT");
                    if (want != *it) { lw.find(want)->data[kDataObjectId] = *it; lw.reindex(*lw.find(want)); }
                }
                std::vector<int16_t> order;
                for (const auto& o : lw.objects()) order.push_back(o->id());
                note(order == L.ids ? "objects: the list's order the same" : "objects: the list's order DIFFERENT");
            }
            const PrimResult r = set_to_next(node, lw, vars);
                const bool orig_true = s.after == raw[2];
                const std::string kind = "SetToNext kind " + std::to_string(node.operand[4] & 0x7F);
                if (r == PrimResult::Stop) { note(kind + ": not modelled"); continue; }
                const bool same = (r == PrimResult::True) == orig_true && (!orig_true || f.stack_object == s.stack_object_after);
                char ex[128];
                std::snprintf(ex, sizeof ex, "tick %u from %d: original %s %d, native %s %d", s.tick, s.stack_object,
                              orig_true ? "true" : "false", s.stack_object_after, r == PrimResult::True ? "true" : "false", f.stack_object);
                // While the lot loads (its first ticks) the original's list is still being built,
                // in another order (a global init tree walks the objects of a GUID in ascending
                // id order then); the native world has the finished list: counted apart.
                note(kind + (same ? ": same" : s.tick < 10 ? ": different while the lot loads (ticks 0-9)" : ": DIFFERENT"), ex);
                if (!same && s.tick >= 10) {
                    static int shown = 0;
                    if (shown++ < 12) {
                        const uint32_t g = (uint32_t)node.operand[0] | node.operand[1] << 8 | node.operand[2] << 16 | (uint32_t)node.operand[3] << 24;
                        int pos_from = -1, pos_orig = -1;
                        for (size_t q = 0; q < lw.objects().size(); q++) {
                            if (lw.objects()[q]->id() == s.stack_object) pos_from = (int)q;
                            if (lw.objects()[q]->id() == s.stack_object_after) pos_orig = (int)q;
                        }
                        std::printf("  %s tick %u tree %#x node %u GUID %08x: from %d (at %d), original %d (at %d, GUID %08x)\n", kind.c_str(), s.tick, s.tree, s.node, g,
                                    s.stack_object, pos_from, s.stack_object_after, pos_orig, lw.find(s.stack_object_after) ? lw.find(s.stack_object_after)->guid : 0);
                    }
                }
            }
        }
    }

    for (const auto& [k, v] : out) {
        std::printf("  %-62s %ld\n", k.c_str(), v);
        if ((k.find("DIFFERENT") != std::string::npos || k[0] == ' ') && example.count(k)) std::printf("      e.g. %s\n", example[k].c_str());
    }
    long diff = 0;
    for (const auto& [k, v] : out) if (k.find("DIFFERENT") != std::string::npos) diff += v;
    std::printf("lockstep: %s (%ld differences, %d objects not found)\n", diff ? "DIFFERENT" : "same", diff, unresolved);
    return diff ? 1 : 0;
}
