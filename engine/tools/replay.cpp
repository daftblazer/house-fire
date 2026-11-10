// The club run natively, tick after tick, against a log of the recompiled Bustin' Out's
// interpreter (its src/tree_log.cpp, BUSTIN_TREE_LOG) and the disc it ran.
//
// Usage: replay LOG ISO
//
// The world is the disc's club lot, loaded natively (the log starts as the club loads):
// each object's saved tree stack, attributes, temps, data, and the Sims' motives and person
// data. The simulator says whose turn it is each tick; the turns are compared with the log's.
// A tick without the pass over the objects is a paused one, whose clock does not move, and
// steps outside the pass are the engine's own (the pie menu's), not run (the log's 'Q'
// records). Each stack is then stepped natively for as many steps as the log has for it in
// that tick,
// and before each step its top frame (depth, tree, node) is compared with the log's. The
// native state is kept from step to step and tick to tick: frames, sleep, the variables.
//
// The primitives the engine has run natively (the expression, Idle, SetToNext) on the
// native world; each variable they read is compared with what the original's lookup found
// before it is used. The others are answered from the log: their result, and their frame's
// parameters, locals and stack object after them.
//
// Where the native state differs from the log's it is counted and then set to the log's, so
// that the run goes on: a frame (an interaction the engine pushed, which is not native yet),
// a variable (one a primitive that is not native wrote). Objects that are not the lot's (the
// household, made as the lot loads, and objects made later) are not run.
#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <cstdio>
#include <cstring>
#include <map>
#include <set>
#include <string>
#include <tuple>

#include "housefire/disc.h"
#include "housefire/expression.h"
#include "housefire/library.h"
#include "housefire/primitives.h"
#include "housefire/save.h"
#include "housefire/simulator.h"
#include "housefire/world.h"
#include "tree_log.h"

using namespace housefire;
using namespace housefire::treelog;

namespace {

std::map<std::string, size_t> counts;
bool quiet = false;  // --bench: no counting, only the timings
void note(const std::string& k, size_t n = 1) { if (!quiet) counts[k] += n; }

// --bench: the native work's time, per tick and per part. The replay's own work (finding
// trees, comparing with the log, setting the native state to the log's) is not counted.
using Clock = std::chrono::steady_clock;
double us(Clock::time_point a, Clock::time_point b) { return std::chrono::duration<double, std::micro>(b - a).count(); }
struct Bench {
    std::vector<double> ticks;  // the native work of each tick that ran, in µs
    double steps_us = 0, expression_us = 0, set_to_next_us = 0, idle_us = 0, pass_us = 0, sync_us = 0;
    size_t steps = 0, worst_tick = 0;
};

// A logged tree on the disc: the file whose tree of that id has those nodes, by the nodes'
// address and the tree's id. The game reuses an unloaded tree's memory for others, and the
// log records a tree only the first time it is at an address, so a step's record can be
// another tree's: only one of the step's own id is used.
struct TreeFinder {
    const Library& lib;
    std::map<std::pair<uint32_t, uint16_t>, std::pair<const Tree*, const ObjectFile*>> cache;
    std::pair<const Tree*, const ObjectFile*> find(const StepRec& s) {
        auto it = cache.find({s.nodes, s.tree});
        if (it != cache.end()) return it->second;
        std::pair<const Tree*, const ObjectFile*> r{nullptr, nullptr};
        if (!s.t || s.t->tree != s.tree) return r;  // not cached: a later step may have its record
        if (s.t)
            for (const auto& f : lib.files()) {
                auto t = f->trees.find(s.tree);
                if (t != f->trees.end() && same_shape(t->second, s.t->raw)) { r = {&t->second, f.get()}; break; }
            }
        if (!r.first && getenv("REPLAY_SHOW_SHAPE") && s.tree == (uint16_t)atoi(getenv("REPLAY_SHOW_SHAPE"))) {
            static int shown = 0;
            if (shown++ < 2) {
                std::printf("tree %d at %08x: %zu nodes logged:", s.tree, s.nodes, s.t->raw.size() / 12);
                for (size_t k = 0; k < s.t->raw.size(); k++) std::printf("%s%02x", k % 12 ? "" : " ", s.t->raw[k]);
                std::printf("\n");
                for (const auto& f : lib.files()) {
                    auto t = f->trees.find(s.tree);
                    if (t == f->trees.end()) continue;
                    std::printf("  %s:", f->path.c_str());
                    for (const Node& n : t->second.nodes) std::printf(" %04x %02x %02x %04x%04x%04x%04x", n.opcode, n.true_target, n.false_target, n.operand16(0), n.operand16(1), n.operand16(2), n.operand16(3));
                    std::printf("\n");
                }
            }
        }
        return cache[{s.nodes, s.tree}] = r;
    }
};

struct Replay;

// An object's host: the native primitives on the native world, the rest from the log step
// being replayed.
struct ReplayHost : Host {
    Replay& r;
    Object& me;
    ReplayHost(Replay& rp, Object& o) : r(rp), me(o) {}
    PrimResult primitive(Interpreter& in, Frame& f, const Node& n) override;
    const Tree* tree(uint16_t id) override;
    std::span<const int16_t> default_params() override { return defaults; }
    std::vector<int16_t> defaults;
};

struct Replay {
    const Library& lib;
    World& world;
    Simulator* sim = nullptr;
    const StepRec* step = nullptr;  // the log's step being replayed
    Bench* bench = nullptr;
    const ObjectFile* file_of_top = nullptr;

    // Brings the native world's variables to what the original's lookups found, counting
    // those that were already the same.
    void sync_variables(Object& me, Frame& f, const StepRec& s) {
        const auto t0 = Clock::now();
        sync_variables_(me, f, s);
        if (bench) bench->sync_us += us(t0, Clock::now());
    }
    void sync_variables_(Object& me, Frame& f, const StepRec& s) {
        ScriptVariables vars(world, me, f);
        for (const LookupRec& v : s.lookups) {
            if (!(v.flags & 1)) continue;  // the lookup found nothing
            const std::string sc = "variables: scope " + std::to_string(v.scope);
            const auto got = vars.get((uint8_t)v.scope, v.data);
            if (!got) { note(sc + ": not modelled"); continue; }
            if (v.flags & 4) {
                float lf;
                std::memcpy(&lf, &v.fvalue, 4);
                if (got->f && *got->f == lf) { note(sc + ": the same natively"); continue; }
                note(sc + ": different (set to the log's)");
                vars.set_float((uint8_t)v.scope, v.data, lf);
                continue;
            }
            if (got->value == v.value) { note(sc + ": the same natively"); continue; }
            static int shown = 0;
            if (getenv("REPLAY_SHOW_SCOPE") && atoi(getenv("REPLAY_SHOW_SCOPE")) == v.scope && shown++ < 10)
                std::printf("scope %d datum %d: native %d, log %d; the step's stack object %d -> %d, tree %d:%d\n", v.scope, v.data,
                            got->value, v.value, s.stack_object, s.stack_object_after, s.tree, s.node);
            note(sc + ": different (set to the log's)");
            vars.set((uint8_t)v.scope, v.data, v.value);
        }
    }
};

const Tree* ReplayHost::tree(uint16_t id) {
    const ObjectFile* f = r.file_of_top ? r.file_of_top : me.file;
    return r.lib.tree(f, id);
}

PrimResult ReplayHost::primitive(Interpreter& in, Frame& f, const Node& n) {
    const StepRec& s = *r.step;
    auto logged = [&] {
        if (s.result == 1) return PrimResult::Yield;
        // 2: a dialog waiting for the player (Dialog, opcode 36, the only one): the node
        // stays and the tick's pass stops after this turn (docs/simulation.md).
        if (s.result == 2) { r.sim->stop_pass(); return PrimResult::Yield; }
        return s.after == n.true_target ? PrimResult::True : PrimResult::False;
    };
    if (n.opcode == 2 || n.opcode == 31) {
        // SetToNext's read of its target variable is logged as 0 whatever it holds (seen with
        // the stack object), so only the expression's reads are taken; SetToNext starts from
        // the frame as synced.
        if (n.opcode == 2) r.sync_variables(me, f, s);
        ScriptVariables vars(r.world, me, f);
        PrimResult got;
        const auto t0 = Clock::now();
        if (n.opcode == 2) got = expression(ExpressionOperand::of(n), vars).result;
        else got = set_to_next(n, r.world, vars);
        if (r.bench) (n.opcode == 2 ? r.bench->expression_us : r.bench->set_to_next_us) += us(t0, Clock::now());
        const PrimResult want = logged();
        const std::string k = n.opcode == 2 ? "primitives: expression" : "primitives: SetToNext";
        if (got == want) note(k + ": the same natively");
        else { note(k + ": DIFFERENT (the log's taken)"); got = want; }
        // The frame as the original's after it (a SetToNext target, a local the expression
        // wrote are checked through the next step's lookups).
        return got;
    }
    if (n.opcode == 0) {  // Idle
        const bool was = in.woke();
        const auto t0 = Clock::now();
        const PrimResult got = idle(in, f, n);
        if (r.bench) r.bench->idle_us += us(t0, Clock::now());
        if (!was && in.woke() && getenv("REPLAY_SHOW_IDLE")) {
            static int shown = 0;
            if (shown++ < 15) std::printf("Idle: object %d at tick %u, %d:%d, parameter %d, operand %d, params", me.id(), r.sim->now(), s.tree, s.node,
                                          n.operand16(0) < f.params.size() ? f.params[n.operand16(0)] : -999, n.operand16(0));
            if (shown <= 15) { for (int16_t p : s.params) std::printf(" %d", p); std::printf("\n"); }
        }
        const PrimResult want = logged();
        if (got == want) note("primitives: Idle: the same natively");
        else note("primitives: Idle: DIFFERENT");
        return got;
    }
    note("primitives: from the log");
    f.params = s.params_after;
    f.locals = s.locals_after;
    f.unset = 0;
    f.stack_object = s.stack_object_after;
    return logged();
}

// The log's stacks: which object each is, from the stack's own variable lookups (scopes 0,
// 3, 8) and the logged object lists.
std::map<uint32_t, int16_t> stack_ids(const Log& log) {
    std::map<uint32_t, int16_t> ptr_id;
    for (const ListRec& l : log.lists)
        for (size_t i = 0; i < l.objects.size(); i++) ptr_id[l.objects[i]] = l.ids[i];
    std::map<uint32_t, std::map<uint32_t, int>> own;
    for (const StepRec& s : log.steps)
        for (const LookupRec& v : s.lookups)
            if (v.scope == 0 || v.scope == 3 || v.scope == 8) own[s.sim][v.object]++;
    std::map<uint32_t, int16_t> out;
    for (const auto& [sim, c] : own) {
        auto best = std::max_element(c.begin(), c.end(), [](auto& a, auto& b) { return a.second < b.second; });
        auto id = ptr_id.find(best->first);
        if (id != ptr_id.end()) out[sim] = id->second;
    }
    return out;
}

// The stacks that never read their own variables, placed: each is a lot object not placed
// yet, between the placed ones it runs after and before in each tick (turns are in id order);
// a group of stacks that can be the same objects, as many as they, takes them in the order
// the group runs in a tick. Repeated until nothing more is placed.
void place_quiet_stacks(const Log& log, const World& world, std::map<uint32_t, int16_t>& ids) {
    std::set<int16_t> free;
    for (const auto& o : world.objects()) free.insert(o->id());
    for (const auto& [sim, id] : ids) free.erase(id);
    std::map<uint32_t, std::set<int16_t>> cand;
    std::map<uint32_t, const StepRec*> first;
    std::map<uint32_t, std::vector<uint32_t>> order;  // tick -> stacks in their order
    for (const StepRec& s : log.steps) {
        first.emplace(s.sim, &s);
        auto& o = order[s.tick];
        if (std::find(o.begin(), o.end(), s.sim) == o.end()) o.push_back(s.sim);
    }
    for (const auto& [sim, f] : first)
        if (!ids.count(sim)) cand[sim] = free;
    for (bool changed = true; changed;) {
        changed = false;
        for (const auto& [t, o] : order)
            for (size_t j = 0; j < o.size(); j++) {
                auto c = cand.find(o[j]);
                if (c == cand.end() || ids.count(o[j])) continue;
                int lo = 0, hi = 1 << 20;
                for (size_t k = 0; k < j; k++) if (ids.count(o[k])) lo = std::max(lo, (int)ids[o[k]]);
                for (size_t k = j + 1; k < o.size(); k++) if (ids.count(o[k])) hi = std::min(hi, (int)ids[o[k]]);
                for (auto it = c->second.begin(); it != c->second.end();) it = (*it <= lo || *it >= hi) ? c->second.erase(it) : std::next(it);
            }
        for (auto& [sim, c] : cand) {
            if (ids.count(sim)) continue;
            for (auto it = c.begin(); it != c.end();) it = free.count(*it) ? std::next(it) : c.erase(it);
            if (c.size() == 1) { ids[sim] = *c.begin(); free.erase(*c.begin()); changed = true; }
        }
        if (changed) continue;
        // A group of stacks with the same candidates, as many as they: in id order, as they
        // take their turns in a tick where all of them run.
        std::map<std::set<int16_t>, std::vector<uint32_t>> groups;
        for (const auto& [sim, c] : cand)
            if (!ids.count(sim) && c.size() > 1) groups[c].push_back(sim);
        for (const auto& [c, g] : groups) {
            if (g.size() != c.size()) continue;
            for (const auto& [t, o] : order) {
                std::vector<uint32_t> seen;
                for (uint32_t sim : o) if (std::find(g.begin(), g.end(), sim) != g.end()) seen.push_back(sim);
                if (seen.size() != g.size()) continue;
                auto id = c.begin();
                for (uint32_t sim : seen) { ids[sim] = *id; free.erase(*id); ++id; }
                changed = true;
                break;
            }
        }
    }
    if (getenv("REPLAY_SHOW_QUIET")) {
        int n = 0;
        for (const auto& [sim, c] : cand)
            if (!ids.count(sim) && n++ < 8) std::printf("unplaced stack: first step %d:%d at tick %u, %zu candidates\n", first[sim]->tree, first[sim]->node, first[sim]->tick, c.size());
        n = 0;
        for (const auto& [sim, c] : cand)
            if (ids.count(sim)) {
                const Object* o = const_cast<World&>(world).find(ids[sim]);
                const bool same = o && !o->stack.empty() && o->stack.back().tree && o->stack.back().tree->id == (int16_t)first[sim]->tree && o->stack.back().node == first[sim]->node;
                note(same ? "stacks: placed, first step the saved top frame" : "stacks: placed, first step not the saved top frame");
            }
        n = 0;
        for (int16_t id : free) {
            const Object* o = const_cast<World&>(world).find(id);
            if (o && !o->stack.empty() && n++ < 8)
                std::printf("free object %d: saved top %d:%d (%zu frames)\n", id, o->stack.back().tree ? o->stack.back().tree->id : -1, o->stack.back().node, o->stack.size());
        }
    }
    size_t placed = 0, left = 0;
    for (const auto& [sim, c] : cand) (ids.count(sim) ? placed : left)++;
    note("stacks: placed by their own variables", ids.size() - placed);
    note("stacks: placed by turn order", placed);
    note("stacks: not placed", left);
}

}  // namespace

// One run of the club over the log: with bench, timed and not counted.
bool run(const Log& log, const Library& lib, const LotChunks& club, Bench* bench) {
    World world(lib);
    world.load_lot(club);
    if (log.lists.empty() || log.lists[0].ids.size() != world.objects().size()) {
        std::printf("the log does not start with the club loaded\n");
        return false;
    }

    Replay rp{lib, world};
    rp.bench = bench;
    Simulator sim(world, [&](Object& o) {
        auto h = std::make_unique<ReplayHost>(rp, o);
        h->defaults.assign(o.temps.begin(), o.temps.end());
        return h;
    });
    rp.sim = &sim;
    TreeFinder finder{lib};
    auto ids = stack_ids(log);
    place_quiet_stacks(log, world, ids);

    // The log's steps by tick, in order.
    std::vector<std::vector<size_t>> by_tick(log.ticks + 1);
    for (size_t i = 0; i < log.steps.size(); i++)
        if (log.steps[i].tick >= 1 && log.steps[i].tick <= log.ticks) by_tick[log.steps[i].tick].push_back(i);
    std::map<uint32_t, std::vector<uint32_t>> frames;  // the log's stacks: frame addresses, bottom first
    std::map<uint32_t, uint32_t> first_seen;           // stack -> tick of its first step
    std::map<std::tuple<uint32_t, uint32_t, uint16_t>, std::pair<const Tree*, const ObjectFile*>> frame_tree;
    size_t streak = 0, best_streak = 0;

    // Which steps are turns: those in the tick's pass over the objects ('Q' records). The
    // others are the engine's own (the pie menu's checks on a Sim's stack: docs/simulation.md).
    // A log without them is taken as every step a turn.
    const bool have_passes = !log.passes.empty();
    std::vector<bool> engine_run(log.steps.size(), false);
    for (size_t i = 0; i < log.steps.size(); i++) engine_run[i] = have_passes && !log.steps[i].in_pass;
    if (!have_passes) note("log: no pass records (an older log): every step taken as a turn, no tick as paused");

    for (uint32_t t = 1; t <= log.ticks; t++) {
        double tick_us = 0;
        const double sync_before = bench ? bench->sync_us : 0;
        // A tick without a pass is a paused one: the native clock does not move in it.
        if (have_passes && !log.passes.count(t)) {
            note("ticks: paused (no pass)");
            for (size_t i : by_tick[t]) (void)i, note("steps: outside the pass (the engine's own), not run");
            continue;
        }
        note("ticks: run");
        // The turns: the native simulator's against the lot objects the log stepped.
        std::vector<int16_t> logged_turns;
        std::set<uint32_t> seen;
        for (size_t i : by_tick[t]) {
            if (engine_run[i]) continue;
            const StepRec& s = log.steps[i];
            auto id = ids.find(s.sim);
            if (id == ids.end() || !world.find(id->second) || !seen.insert(s.sim).second) continue;
            logged_turns.push_back(id->second);
        }
        // The native pass: from its cursor (against where the log's started), over the
        // objects the log's reached (to where it stopped), sleepers passed over.
        const PassRec pass = have_passes ? log.passes.at(t) : PassRec{};
        if (have_passes) note(sim.cursor() == (int16_t)pass.from ? "passes: the native cursor where the log's pass starts" : "passes: DIFFERENT start");
        std::vector<int16_t> native_turns;
        const auto p0 = Clock::now();
        for (auto [o, asleep] : sim.pass_order()) {
            if (pass.stopped_at && o->id() > (int16_t)pass.stopped_at) break;
            if (asleep) sim.pass_over(*o);
            else native_turns.push_back(o->id());
        }
        if (bench) { const double d = us(p0, Clock::now()); bench->pass_us += d; tick_us += d; }
        std::vector<int16_t> a = logged_turns, b = native_turns;
        std::sort(a.begin(), a.end());
        std::sort(b.begin(), b.end());
        if (a == b) note("turns: the same objects");
        else {
            std::vector<int16_t> only_log, only_native;
            std::set_difference(a.begin(), a.end(), b.begin(), b.end(), std::back_inserter(only_log));
            std::set_difference(b.begin(), b.end(), a.begin(), a.end(), std::back_inserter(only_native));
            note("turns: different objects");
            note("turns: objects the log ran, not the native", only_log.size());
            note("turns: objects the native would run, not the log", only_native.size());
            for (int16_t id : only_native) {
                Object* o = world.find(id);
                const auto& st = sim.stack_of(*o).stack();
                const Node* n = !st.empty() && st.back().tree && st.back().node < st.back().tree->nodes.size() ? &st.back().tree->nodes[st.back().node] : nullptr;
                note("turns: the native would run, at opcode " + (n ? std::to_string(n->opcode) : std::string("?")) + (sim.stack_of(*o).woke() ? " (slept)" : ""));
            }
            static int shown = 0;
            if (getenv("REPLAY_SHOW_TURNS") && shown < 10) {
                for (int16_t id : only_log) {
                    Interpreter& in = sim.stack_of(*world.find(id));
                    std::printf("  %d: done %d asleep %d depth %zu error %d\n", id, in.done(), in.asleep(), in.stack().size(), (int)in.error());
                }
            }
            if (getenv("REPLAY_SHOW_TURNS") && shown++ < 10) {
                std::printf("tick %u turns differ: only the log's", t);
                for (int16_t id : only_log) std::printf(" %d", id);
                std::printf("; only the native's");
                for (int16_t id : only_native) std::printf(" %d", id);
                std::printf("\n");
            }
        }

        // The steps, in the log's order.
        for (size_t i : by_tick[t]) {
            const StepRec& s = log.steps[i];
            if (engine_run[i]) { note("steps: outside the pass (the engine's own), not run"); continue; }
            auto& fr = frames[s.sim];
            auto at = std::find(fr.begin(), fr.end(), s.frame);
            if (at != fr.end()) fr.erase(at + 1, fr.end());
            else fr.push_back(s.frame);
            first_seen.emplace(s.sim, t);
            auto id = ids.find(s.sim);
            Object* o = id == ids.end() ? nullptr : world.find(id->second);
            if (!o) { note("steps: not a lot object's (not run)"); continue; }
            Interpreter& in = sim.stack_of(*o);
            // A return step's nodes are its caller's (the lookup of where to go back to): its
            // tree is the frame's, from the frame's earlier steps.
            std::pair<const Tree*, const ObjectFile*> tf{nullptr, nullptr};
            const auto key = std::make_tuple(s.sim, s.frame, s.tree);
            if (s.look == 'R') {
                auto ft = frame_tree.find(key);
                if (ft != frame_tree.end()) tf = ft->second;
            } else {
                tf = finder.find(s);
                if (tf.first) frame_tree[key] = tf;
            }
            const auto [tree, file] = tf;
            if (!tree) {
                note("steps: tree not found on the disc");
                note(std::string("steps: tree not found: ") + (s.t ? "tree " : "no tree record, tree ") + std::to_string(s.tree));
                continue;
            }
            // The native stack before the step against the log's.
            const auto& st = in.stack();
            const bool same = !in.done() && st.size() == fr.size() && st.back().tree && st.back().tree->id == (int16_t)s.tree && st.back().node == s.node;
            if (same) {
                note("steps: the native stack as the log's");
                best_streak = std::max(best_streak, ++streak);
            } else {
                streak = 0;
                if (in.done()) note("steps: different: the native stack stopped");
                else if (st.size() != fr.size()) note(st.size() < fr.size() ? "steps: different: the log's is deeper (a frame pushed)" : "steps: different: the native is deeper");
                else if (!st.back().tree || st.back().tree->id != (int16_t)s.tree) {
                    note("steps: different: another tree");
                    static int shown = 0;
                    if (getenv("REPLAY_SHOW_TREES") && shown++ < 12)
                        std::printf("tick %u object %d: native %d:%d (depth %zu), log %d:%d (depth %zu), log step before: %s\n", t, o->id(),
                                    st.back().tree ? st.back().tree->id : -1, st.back().node, st.size(), s.tree, s.node, fr.size(),
                                    i > 0 && log.steps[i - 1].sim == s.sim ? (std::to_string(log.steps[i - 1].tree) + ":" + std::to_string(log.steps[i - 1].node) + "->" + std::to_string(log.steps[i - 1].after)).c_str() : "another stack's");
                }
                else note("steps: different: another node");
                // Set to the log's: the frames below kept, the top one the log's.
                std::vector<Frame> nf(st.begin(), st.begin() + (long)std::min(st.size(), fr.size() - 1));
                while (nf.size() < fr.size() - 1) nf.push_back(Frame{});  // frames the log has, not known natively
                Frame top;
                top.tree = tree;
                top.node = s.node;
                top.params = s.params;
                top.locals = s.locals;
                top.stack_object = s.stack_object;
                top.file = file;
                nf.push_back(top);
                in.restore(std::move(nf));
                if (in.asleep()) in.wake();
            }
            if (same) {
                // The frame's own state as the native kept it, against the log's at the step.
                Frame& f = const_cast<Frame&>(in.stack().back());
                note(f.params == s.params ? "frames: parameters the same natively" : "frames: parameters different (set to the log's)");
                note(f.locals == s.locals ? "frames: locals the same natively" : "frames: locals different (set to the log's)");
                note(f.stack_object == s.stack_object ? "frames: stack object the same natively" : "frames: stack object different (set to the log's)");
                if (f.stack_object != s.stack_object && i > 0) {
                    const StepRec& p = log.steps[i - 1];
                    const bool own = p.sim == s.sim;
                    note(std::string("frames: stack object different, after ") +
                         (!own ? "another stack's step" : p.frame != s.frame ? (p.frame < s.frame ? "a call" : "a return") : "a step of the frame"));
                    static int shown = 0;
                    if (getenv("REPLAY_SHOW_SO") && shown++ < 12)
                    {
                        std::printf("stack object: native %d, log %d at %d:%d; before: %d:%d->%d (%c) so %d->%d%s", f.stack_object, s.stack_object, s.tree, s.node,
                                    p.tree, p.node, p.after, p.look ? p.look : '-', p.stack_object, p.stack_object_after, own ? "" : " (another stack)");
                        if (p.has_expr) std::printf(" expr lhs %d/%d rhs %d/%d op %d lhs %d->%d rhs %d", p.expr.lhs_scope, p.expr.lhs_data, p.expr.rhs_scope, p.expr.rhs_data, p.expr.op, p.expr.lhs, p.expr.lhs_after, p.expr.rhs);
                        for (const LookupRec& v : p.lookups) std::printf(" [V %d/%d=%d fl %d]", v.scope, v.data, v.value, v.flags);
                        std::printf("\n");
                    }
                }
                f.params = s.params;
                f.locals = s.locals;
                f.stack_object = s.stack_object;
            }
            if (in.asleep()) { note("steps: the log ran a stack the native has asleep"); in.wake(); }
            rp.step = &s;
            rp.file_of_top = in.stack().back().file;
            in.reset_iterations();
            const auto s0 = Clock::now();
            in.step();
            if (bench) { const double d = us(s0, Clock::now()); bench->steps_us += d; tick_us += d; bench->steps++; }
        }
        if (have_passes) note(sim.stop_asked() == (pass.stopped_at != 0) ? "passes: stopped (or not) as the log's" : "passes: DIFFERENT stop");
        sim.end_pass(have_passes ? (int16_t)pass.stopped_at : 0);
        if (bench) {
            tick_us -= bench->sync_us - sync_before;  // the replay's own, inside the steps
            if (bench->ticks.empty() || tick_us > bench->ticks[bench->worst_tick]) bench->worst_tick = bench->ticks.size();
            bench->ticks.push_back(tick_us);
        }
    }

    if (getenv("REPLAY_SHOW_FILES")) {
        std::map<std::string, int> f;
        for (const StepRec& s : log.steps) {
            auto id = ids.find(s.sim);
            Object* o = id == ids.end() ? nullptr : world.find(id->second);
            if (!o || !o->person) continue;
            const auto tf = finder.find(s);
            if (tf.second && tree_kind(s.tree) == TreeKind::Own) f[tf.second->path]++;
        }
        for (auto& [p, n] : f) std::printf("a lot Sim's own trees in %s: %d steps\n", p.c_str(), n);
    }
    if (!bench) note("steps: longest run with the native stack as the log's", best_streak);
    return true;
}

int main(int argc, char** argv) {
    if (argc < 3) { std::printf("usage: replay LOG ISO\n"); return 2; }
    Log log;
    if (!read_log(argv[1], log)) { std::printf("cannot read %s\n", argv[1]); return 1; }
    Disc d(argv[2]);
    if (!d.ok()) { std::printf("cannot read %s\n", argv[2]); return 1; }
    const Library lib = Library::bustin_out(d);
    const auto lots = read_lots(d.read("Runtime/default.ngh"));
    const bool bench = argc > 3 && std::string(argv[3]) == "--bench";
    if (!bench) {
        if (!run(log, lib, lots.back(), nullptr)) return 1;
        for (const auto& [k, n] : counts) std::printf("  %-66s %zu\n", k.c_str(), n);
        return 0;
    }
    // The benchmark: five runs, each figure the median of the runs (the PC is shared).
    quiet = true;
    std::vector<Bench> runs(5);
    for (Bench& b : runs) if (!run(log, lib, lots.back(), &b)) return 1;
    auto median = [&](auto f) {
        std::vector<double> v;
        for (const Bench& b : runs) v.push_back(f(b));
        std::sort(v.begin(), v.end());
        return v[v.size() / 2];
    };
    auto pct = [](std::vector<double> v, double q) { std::sort(v.begin(), v.end()); return v[(size_t)(q * (double)(v.size() - 1))]; };
    const size_t ticks = runs[0].ticks.size();
    std::printf("bench: %zu ticks, %zu native steps (%.1f a tick), 5 runs, medians\n", ticks, runs[0].steps, (double)runs[0].steps / (double)ticks);
    std::printf("  per tick:  mean %.2f us, median %.2f us, 99th percentile %.2f us, worst %.2f us\n",
                median([&](const Bench& b) { double t = 0; for (double x : b.ticks) t += x; return t / (double)b.ticks.size(); }),
                median([&](const Bench& b) { return pct(b.ticks, 0.5); }), median([&](const Bench& b) { return pct(b.ticks, 0.99); }),
                median([&](const Bench& b) { return b.ticks[b.worst_tick]; }));
    std::printf("  the worst tick: tick %zu; the worst from tick 10 on (past the load): %.2f us\n", runs[0].worst_tick + 1,
                median([&](const Bench& b) { return *std::max_element(b.ticks.begin() + std::min<size_t>(9, b.ticks.size() - 1), b.ticks.end()); }));
    std::printf("  per part, us a tick: scripts (steps) %.2f, of which expression %.2f, SetToNext %.2f, Idle %.2f; the pass %.2f\n",
                median([&](const Bench& b) { return (b.steps_us - b.sync_us) / (double)b.ticks.size(); }),
                median([&](const Bench& b) { return b.expression_us / (double)b.ticks.size(); }),
                median([&](const Bench& b) { return b.set_to_next_us / (double)b.ticks.size(); }),
                median([&](const Bench& b) { return b.idle_us / (double)b.ticks.size(); }),
                median([&](const Bench& b) { return b.pass_us / (double)b.ticks.size(); }));
    std::printf("  not native yet (not in these figures): routing, autonomy, animation, the other primitives\n");
    return 0;
}
