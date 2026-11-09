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
#include <span>
#include <string>

#include "housefire/disc.h"
#include "housefire/interp.h"

using namespace housefire;

namespace {

struct File {
    std::string name;
    std::vector<uint8_t> bytes;
    IffFile iff;
    std::map<uint16_t, Tree> trees;
    std::string glob;
    const File* semi = nullptr;
};

std::string lower(std::string s) {
    for (char& c : s) c = (char)std::tolower((unsigned char)c);
    return s;
}

// A GLOB names the semi-global file: as a length-prefixed string ("\rPersonGlobals", the
// characters' files) or a zero-ended one ("person_sg", the objects').
std::string glob_name(std::span<const uint8_t> d) {
    if (d.empty()) return {};
    if (d[0] < 0x20) return d[0] + 1u <= d.size() ? std::string((const char*)d.data() + 1, d[0]) : std::string();
    size_t n = 0;
    while (n < d.size() && d[n]) n++;
    return std::string((const char*)d.data(), n);
}

// ---- The log ----

struct TreeRec { uint32_t nodes; uint16_t tree; uint32_t file; std::vector<uint8_t> raw; };
struct StepRec {
    uint32_t sim, frame, file, nodes;
    uint16_t tree, node, after, error;
    uint8_t look, result;
    std::vector<int16_t> params, locals, params_after, locals_after;
    int16_t defaults[8] = {};  // the interpreter's own parameters (its +44), for calls giving none
    const TreeRec* t = nullptr;  // the tree GetNodeRef found, as loaded at the time
};

struct Log {
    std::vector<std::unique_ptr<TreeRec>> trees;
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
    while (p < b.size()) {
        const char k = (char)b[p++];
        if (k == 'K') { if (p + 4 > b.size()) break; u32(); log.ticks++; }
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
            if (p + 16 > b.size()) break;
            for (int i = 0; i < 8; i++) s.defaults[i] = (int16_t)u16();
            auto it = at.find(s.nodes);
            if (it != at.end()) s.t = it->second;
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

struct Lookup {
    const File* own = nullptr;
    const File* global = nullptr;
    const Tree* find(uint16_t id) const {
        const File* f = nullptr;
        switch (tree_kind(id)) {
            case TreeKind::Global: f = global; break;
            case TreeKind::Own: f = own; break;
            case TreeKind::SemiGlobal: f = own ? own->semi : nullptr; break;
            default: return nullptr;
        }
        if (!f) return nullptr;
        auto it = f->trees.find(id);
        return it == f->trees.end() ? nullptr : &it->second;
    }
};

// Answers the one primitive of the step as the original did.
struct LogHost : Host {
    Lookup lookup;
    PrimResult answer = PrimResult::Stop;
    std::vector<int16_t> defaults;
    PrimResult primitive(Interpreter&, Frame&, const Node&) override { return answer; }
    std::span<const int16_t> default_params() override { return defaults; }
    const Tree* tree(uint16_t id) override { return lookup.find(id); }
};

struct FrameState {
    uint16_t tree, node;
    std::vector<int16_t> params, locals;
    const struct File* object;  // each frame its own: a Sim runs an object's interaction on its stack
};

}  // namespace

int main(int argc, char** argv) {
    if (argc < 3) { std::printf("usage: lockstep LOG ISO\n"); return 2; }
    Log log;
    if (!read_log(argv[1], log)) { std::printf("cannot read %s\n", argv[1]); return 1; }
    Disc d(argv[2]);
    if (!d.ok() || d.id() != "G4ME69") { std::printf("%s is not Bustin' Out\n", argv[2]); return 1; }

    std::vector<std::unique_ptr<File>> all;
    std::map<std::string, File*> by_name;
    for (const auto& [path, where] : d.files()) {
        if (path.rfind("Runtime/", 0) != 0 || path.size() < 5 || path.substr(path.size() - 4) != ".iff") continue;
        auto f = std::make_unique<File>();
        f->name = path;
        f->bytes = d.read(path);
        f->iff = read_iff(f->bytes);
        const Order o = bhav_order(f->iff);
        for (const Chunk* c : f->iff.of("BHAV")) {
            try { f->trees[(uint16_t)c->id] = read_bhav(*c, o); } catch (const FormatError&) {}
        }
        for (const Chunk* c : f->iff.of("GLOB")) f->glob = glob_name(c->data);
        const size_t slash = path.rfind('/');
        by_name.emplace(lower(path.substr(slash + 1, path.size() - slash - 5)), f.get());
        all.push_back(std::move(f));
    }
    // The characters' files name PersonGlobals, but the game runs trees for them that only
    // person_sg has (seen in the log: 0x2032, 0x204F, ...): their semi-global file is
    // person_sg, as template_person's GLOB names. PersonGlobals.iff is not used.
    for (auto& f : all) {
        std::string g = lower(f->glob);
        if (g == "personglobals") g = "person_sg";
        if (!g.empty() && by_name.count(g)) f->semi = by_name[g];
    }
    const File* global = by_name.count("global") ? by_name["global"] : nullptr;

    // Each object's file: the disc file under which every tree the original ran for it is
    // found with the same nodes. Memory is reused, so a file pointer's trees are taken in
    // order and split where no one disc file has them all (another object at that address);
    // each part is an object.
    auto fits = [&](const File* f, const TreeRec* t) {
        const Tree* nt = Lookup{f, global}.find(t->tree);
        return nt && same_shape(*nt, t->raw);
    };
    // The sightings are the steps: each names its object's file pointer and the tree run (a
    // tree is logged once, under the first object that ran it, but every step has its own).
    struct Part { std::vector<const File*> files; };
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
            std::vector<const File*> keep;
            for (const File* f : parts[it->second].files) if (fits(f, s.t)) keep.push_back(f);
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
    std::vector<const File*> step_object(log.steps.size(), nullptr);
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
        h.lookup = {step_object[i], global};
        h.defaults.assign(s.defaults, s.defaults + 8);
        std::vector<Frame> native;
        bool ok = true;
        for (uint32_t fp : st) {
            const FrameState& fs = frames[fp];
            Frame f;
            f.tree = fs.object ? Lookup{fs.object, global}.find(fs.tree) : nullptr;
            if (!f.tree) { ok = false; break; }
            f.node = fs.node;
            f.params = fs.params;
            f.locals = fs.locals;
            native.push_back(std::move(f));
        }
        if (!ok) {
            std::string e = "object " + step_object[i]->name + ", stack:";
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
    for (const auto& [k, v] : out) {
        std::printf("  %-62s %ld\n", k.c_str(), v);
        if ((k.find("DIFFERENT") != std::string::npos || k[0] == ' ') && example.count(k)) std::printf("      e.g. %s\n", example[k].c_str());
    }
    long diff = 0;
    for (const auto& [k, v] : out) if (k.find("DIFFERENT") != std::string::npos) diff += v;
    std::printf("lockstep: %s (%ld differences, %d objects not found)\n", diff ? "DIFFERENT" : "same", diff, unresolved);
    return diff ? 1 : 0;
}
