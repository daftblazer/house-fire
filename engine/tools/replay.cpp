// The club run natively, tick after tick, against a log of the recompiled Bustin' Out's
// interpreter (its src/tree_log.cpp, BUSTIN_TREE_LOG) and the disc it ran.
//
// Usage: replay LOG ISO [--card CARD] [--lot N] [--bench]
//
// --card: the lot from a save on a memory card image (its first lot, Mom's house in the
// user's save, unless --lot says), not the disc's club; the neighbours are the save's too.
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
#include <functional>
#include <cmath>
#include <chrono>
#include <cstdlib>
#include <cstdio>
#include <cstring>
#include <map>
#include <set>
#include <string>
#include <tuple>

#include "housefire/autonomy.h"
#include "housefire/disc.h"
#include "housefire/expression.h"
#include "housefire/library.h"
#include "housefire/motives.h"
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
    double steps_us = 0, expression_us = 0, set_to_next_us = 0, idle_us = 0, other_us = 0, pass_us = 0, sync_us = 0;
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
    std::optional<bool> run_check(const Tree* t, const ObjectFile* file, int16_t stack_object) override;
    std::vector<int16_t> defaults;
    std::function<void(bool)> on_check;  // told each finished check's result (the evaluation's comparison)
};

struct Replay {
    const Library& lib;
    World& world;
    Simulator* sim = nullptr;
    const StepRec* step = nullptr;  // the log's step being replayed
    Bench* bench = nullptr;
    const ObjectFile* file_of_top = nullptr;
    // The log's steps of the checks a primitive runs (the interaction queue's), in order.
    const Log* log = nullptr;
    std::vector<size_t> nested;
    size_t nested_pos = 0;
    std::set<int16_t> natively_delivered;
    std::set<int16_t> present;  // the objects on the lot this tick (the log's list)
    std::set<size_t> deferred;  // nested records already used out of order (calls')
    bool card_lot = false;      // the lot is a save's (--card)
    size_t score_at = 0;        // the next logged score ('A')

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

// A check tree the engine runs, stepped natively through the log's steps of it.
std::optional<bool> ReplayHost::run_check(const Tree* t, const ObjectFile* file, int16_t stack_object) {
    Interpreter chk(*this, Interpreter::Mode::Check);
    chk.start(t, {}, file);
    if (chk.done()) return std::nullopt;
    chk.top().stack_object = stack_object;
    const StepRec* outer = r.step;
    const ObjectFile* outer_file = r.file_of_top;
    while (!chk.done()) {
        // A record for a call that ran its callee's steps first is past them: skip it here.
        while (r.nested_pos < r.nested.size() && r.deferred.count(r.nested[r.nested_pos])) r.nested_pos++;
        // A node whose step calls a tree (CallNamedTree, opcode 28) is logged when it ends,
        // after its callee's steps: its record is the next one with its tree and node.
        {
            const Frame& top = chk.top();
            const Node* tn = top.tree && top.node < top.tree->nodes.size() ? &top.tree->nodes[top.node] : nullptr;
            if (tn && tn->opcode == 28) {
                size_t k = r.nested_pos;
                while (k < r.nested.size() && !(r.log->steps[r.nested[k]].tree == (uint16_t)top.tree->id && r.log->steps[r.nested[k]].node == top.node)) k++;
                if (k < r.nested.size() && k != r.nested_pos) {
                    const StepRec& cs = r.log->steps[r.nested[k]];
                    r.deferred.insert(r.nested[k]);
                    note("queue: a check's call step as the log's (its record after its callee's)");
                    r.step = &cs;
                    r.file_of_top = top.file;
                    chk.reset_iterations();
                    chk.step();
                    continue;
                }
            }
        }
        if (r.nested_pos >= r.nested.size()) {
            note("queue: a check ran past the log's steps of it");
            if (getenv("REPLAY_SHOW_PUSH")) std::printf("past: Sim %d at %d:%d depth %zu\\n", me.id(), chk.top().tree ? chk.top().tree->id : -1, chk.top().node, chk.stack().size());
            break;
        }
        const StepRec& ns = r.log->steps[r.nested[r.nested_pos++]];
        const Frame& top = chk.top();
        if (getenv("REPLAY_TRACE_CHECK") && me.id() == 169)
            std::printf("  check %d: native %d:%d depth %zu; log %d:%d->%d %c\n", t->id, top.tree ? top.tree->id : -1, top.node, chk.stack().size(), ns.tree, ns.node, ns.after, ns.look ? ns.look : '-');
        const bool same = top.tree && top.tree->id == (int16_t)ns.tree && top.node == ns.node;
        note(same ? "queue: a check's step as the log's" : "queue: a check's step DIFFERENT");
        if (!same && getenv("REPLAY_SHOW_PUSH"))
            std::printf("check of Sim %d: native %d:%d depth %zu, log %d:%d (so %d, look %c, res %d, after %d)\n", me.id(), top.tree ? top.tree->id : -1, top.node,
                        chk.stack().size(), ns.tree, ns.node, ns.stack_object, ns.look ? ns.look : '-', ns.result, ns.after);
        r.step = &ns;
        r.file_of_top = top.file;
        chk.reset_iterations();
        chk.step();
    }
    r.step = outer;
    r.file_of_top = outer_file;
    if (getenv("REPLAY_TRACE_CHECK")) std::printf("  check %d done %d error %d result %d\n", t->id, chk.done(), (int)chk.error(), chk.result());
    const std::optional<bool> result = chk.done() && chk.error() == Error::None ? std::optional<bool>(chk.result()) : std::nullopt;
    if (result && on_check) on_check(*result);
    return result;
}

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
    if (n.opcode == 8) {  // Random: the engine's own numbers; the result's range checked, the log's value taken
        r.sync_variables(me, f, s);
        ScriptVariables vars(r.world, me, f);
        const PrimResult got = random(n, r.world, vars);
        const PrimResult want = logged();
        note(got == want ? "primitives: Random: the result the same natively" : "primitives: Random: DIFFERENT result");
        // The original's number, where the log shows it (a parameter or local destination):
        // within the range, and then set natively so the run goes on with it.
        const uint8_t dscope = n.operand[2];
        const int datum = n.operand[0] | n.operand[1] << 8;
        const auto range = vars.get(n.operand[6], (int16_t)(n.operand[4] | n.operand[5] << 8));
        const std::vector<int16_t>* after = dscope == 25 ? &s.locals_after : dscope == 9 ? &s.params_after : nullptr;
        if (after && range && datum < (int)after->size()) {
            const int16_t v = (*after)[(size_t)datum];
            note(v >= 0 && (range->value <= 0 ? v == 0 : v < range->value) ? "primitives: Random: the original's number within the range" : "primitives: Random: the original's number OUT of the range");
            vars.set(dscope, (int16_t)datum, v);
        } else note("primitives: Random: the original's number not in the log (a temp or attribute)");
        return want;
    }
    if (n.opcode == 17 && me.person && !me.queue.empty() && !r.nested.empty()) {  // IdleForInput delivering an entry
        if (getenv("REPLAY_TRACE_CHECK")) std::printf("IdleForInput natively for Sim %d, %zu nested steps, at %zu\n", me.id(), r.nested.size(), r.nested_pos);
        const PrimResult got = idle_for_input(in, n, me, r.world, *this);
        if (getenv("REPLAY_TRACE_CHECK")) std::printf("  -> %d, used %zu\n", (int)got, r.nested_pos);
        note(got == PrimResult::Continue ? "queue: IdleForInput delivered the queued interaction natively" : "queue: IdleForInput natively did NOT deliver");
        if (got == PrimResult::Continue) r.natively_delivered.insert(me.id());
        return got == PrimResult::Continue ? got : logged();
    }
    if (n.opcode == 3 && me.person && !r.nested.empty()) {  // the autonomy evaluation
        std::vector<Candidate> cands;
        for (const Candidate& c : autonomy_candidates(r.world, me))
            if (r.present.empty() || r.present.count(c.object->id())) cands.push_back(c);
        // The engine's evaluation, each check's result against the log's: the step that ends
        // it (its bottom frame's return, 254 true or 255 false) is the last nested step used.
        size_t same = 0, different = 0;
        on_check = [&](bool got) {
            if (r.nested_pos == 0) return;
            const StepRec& end = r.log->steps[r.nested[r.nested_pos - 1]];
            (got == (end.node == 254) ? same : different)++;
        };
        if (getenv("REPLAY_SHOW_AUTONOMY")) {
            std::printf("evaluation by %d: %zu candidates, present %zu; objects", me.id(), cands.size(), r.present.size());
            int16_t lastid = -1;
            for (const Candidate& c : cands) if (c.object->id() != lastid) { lastid = c.object->id(); if (lastid > 190) std::printf(" %d", lastid); }
            std::printf("; 197 present %d, in world %d\n", (int)r.present.count(197), r.world.find(197) != nullptr);
        }
        const auto checks = run_evaluation(r.world, me, *this, cands);
        on_check = nullptr;
        size_t unfinished = 0;
        for (const EvaluationCheck& c : checks) unfinished += !c.passed.has_value();
        std::optional<int16_t> native_choice;
        {
            const auto curves = score_curves(r.lib);
            const float base = interaction_score(curves, nullptr, me, me.motives);
            std::vector<std::pair<float, const Candidate*>> values;
            size_t k = 0, scores_same = 0, scores_diff = 0;
            for (const EvaluationCheck& c : checks) {
                if (!c.candidate || !c.passed || !*c.passed) continue;
                // The log's score of this one (the scores come in the order of the passing guards).
                const float native_score = interaction_score(curves, &c.candidate->interaction->adverts, me, me.motives);
                while (r.score_at < r.log->scores.size() && r.log->scores[r.score_at].tick < r.step->tick) r.score_at++;
                if (r.score_at < r.log->scores.size() && r.log->scores[r.score_at].tick == r.step->tick) {
                    (native_score == r.log->scores[r.score_at].score ? scores_same : scores_diff)++;
                    r.score_at++;
                }
                values.push_back({candidate_value(r.lib, curves, me, *c.candidate, base), c.candidate});
                k++;
            }
            note("autonomy: a candidate's score the same natively (its own adverts, the native motives)", scores_same);
            if (scores_diff) note("autonomy: a candidate's score DIFFERENT", scores_diff);
            // The list and the choice natively (autonomy_list, autonomy_pick), against the
            // original's sorted list ('Y', the tick's last) and with its random number ('R').
            {
                std::vector<Valued> vs;
                for (const auto& [v, c] : values) vs.push_back({v, c});
                const auto list = autonomy_list(r.lib, vs, false);
                const ChoiceListRec* logged_list = nullptr;
                for (const ChoiceListRec& l : r.log->choice_lists) if (l.tick == r.step->tick) logged_list = &l;
                const RandomRec* rnd = nullptr;
                for (const RandomRec& x : r.log->randoms) if (x.tick == r.step->tick) rnd = &x;
                if (logged_list) {
                    // The list's interaction number is the interaction's string index.
                    auto index_of = [&](const Candidate& c) { return c.interaction->string_index; };
                    bool same = logged_list->entries.size() == list.size();
                    size_t flags_same = 0, flags_diff = 0;
                    for (size_t i = 0; i < list.size() && i < logged_list->entries.size(); i++) {
                        const auto& e = logged_list->entries[i];
                        same = same && e.id == list[i].candidate->object->id() && (int)e.interaction == index_of(*list[i].candidate) && e.value == list[i].value;
                        if (e.takes_best) flags_diff++; else flags_same++;
                    }
                    note(same ? "autonomy: the choice's list the same natively (objects, interactions, values bit for bit)" : "autonomy: the choice's list DIFFERENT");
                    note("autonomy: the list's entries without the takes-best flag", flags_same);
                    if (flags_diff) note("autonomy: the list's entries WITH the takes-best flag (not built)", flags_diff);
                    if (!same && getenv("REPLAY_SHOW_AUTONOMY")) {
                        std::printf("list at tick %u: log %zu, native %zu\n", r.step->tick, logged_list->entries.size(), list.size());
                        for (size_t i = 0; i < std::max(list.size(), logged_list->entries.size()); i++) {
                            if (i < logged_list->entries.size()) { const auto& e = logged_list->entries[i]; std::printf("  log %d/%u=%.9g", e.id, e.interaction, e.value); } else std::printf("  log -");
                            if (i < list.size() && i < logged_list->entries.size() && getenv("REPLAY_SHOW_DISTANCE")) {
                                const Candidate& c = *list[i].candidate;
                                const float sc = interaction_score(curves, &c.interaction->adverts, me, me.motives);
                                const float att = attenuation(r.lib, *c.interaction, false);
                                const float implied = att > 0 ? ((sc - base) / logged_list->entries[i].value - 1.0f) / att : -1.0f;
                                std::printf(" [d %.4f implied %.4f att %.2f me %d,%d,%d obj %d,%d,%d]", person_distance(me, *c.object), implied, att, me.x16, me.y16, me.level, c.object->x16, c.object->y16, c.object->level);
                            }
                            if (i < list.size()) std::printf("   native %d/%d(s%d,f%x)=%.9g\n", list[i].candidate->object->id(), index_of(*list[i].candidate), list[i].candidate->interaction->string_index, list[i].candidate->interaction->flags, list[i].value); else std::printf("   native -\n");
                        }
                    }
                }
                if (rnd) {
                    const auto at = autonomy_pick(r.lib, list, rnd->value);
                    const bool ok = at && list[*at].candidate->object->id() == s.stack_object_after;
                    note(ok ? "autonomy: the native choice (the original's random number) the log's" : "autonomy: the native choice (the original's random number) NOT the log's");
                    if (getenv("REPLAY_SHOW_AUTONOMY")) std::printf("pick at tick %u: random %u, index %d, native %d, log %d\n", r.step->tick, rnd->value, at ? (int)*at : -1, at ? list[*at].candidate->object->id() : -1, s.stack_object_after);
                    if (ok) native_choice = list[*at].candidate->object->id();
                }
            }
            std::stable_sort(values.begin(), values.end(), [](const auto& a, const auto& b) { return a.first > b.first; });
            size_t rank = 0;
            for (size_t i = 0; i < values.size(); i++)
                if (values[i].second->object->id() == s.stack_object_after) { rank = i + 1; break; }
            note(rank >= 1 && rank <= 4 ? "autonomy: the log's choice among the native best four" : "autonomy: the log's choice NOT among the native best four");
            if (getenv("REPLAY_SHOW_AUTONOMY")) {
                std::printf("choice of %d: object %d, native rank %zu of %zu; best:", me.id(), s.stack_object_after, rank, values.size());
                for (size_t i = 0; i < values.size() && i < 4; i++) std::printf(" %d/%d=%.6g", values[i].second->object->id(), values[i].second->interaction->action, values[i].first);
                std::printf("\n");
            }
        }
        note("autonomy: an evaluation's checks, result as the log's", same);
        note("autonomy: an evaluation's checks, result DIFFERENT", different);
        if (unfinished) note("autonomy: an evaluation's checks, not finished", unfinished);
        // The choice: the native one, made with the original's random number (the engine's
        // own numbers differ, docs/plan.md); the log's where there is none.
        f.stack_object = native_choice ? *native_choice : s.stack_object_after;
        if (native_choice) note("autonomy: an evaluation's result (the stack object) made natively");
        return logged();
    }
    if (n.opcode == 29) {  // SetMotiveDelta
        ScriptVariables vars(r.world, me, f);
        if (auto got = set_motive_delta(n, me, vars)) {
            note(*got == logged() ? "primitives: SetMotiveDelta the same natively" : "primitives: SetMotiveDelta: DIFFERENT");
            return *got;
        }
        note("primitives: SetMotiveDelta: an influence, the log's taken");
    }
    if (n.opcode == 36) {  // Dialog: the replay plays the player, answering when the original's did
        if (s.result != 2) r.world.dialog_answer = s.after == n.true_target;
        const PrimResult got = dialog(n, r.world, me);
        r.world.presentation.clear();
        const bool same = s.result == 2 ? got == PrimResult::Yield && r.world.stop_pass : got == logged();
        note(same ? (s.result == 2 ? "primitives: Dialog: waiting, as the log's" : "primitives: Dialog: answered, as the log's") : "primitives: Dialog: DIFFERENT");
        return got;
    }
    if (n.opcode == 13) {  // PushAction
        const PrimResult got = push_action(in, f, n, r.world);
        note(got == logged() ? "queue: PushAction the same natively" : got == PrimResult::Stop ? "queue: PushAction not modelled (the log's taken)" : "queue: PushAction DIFFERENT");
        return logged();
    }
    if (n.opcode == 23 || n.opcode == 7 || n.opcode == 48) {  // the presentation's requests
        const PrimResult got = present(n, r.world, me);
        r.world.presentation.clear();  // no front end here
        note(got == logged() ? "primitives: PlaySound/Update/KillSounds: the same natively" : "primitives: PlaySound/Update/KillSounds: DIFFERENT (the log's taken)");
        return logged();
    }
    if (n.opcode == 28) {  // CallNamedTree
        const PrimResult got = call_named_tree(in, n, f, r.world);
        if (got == PrimResult::Continue) { note("primitives: CallNamedTree: a tree called natively"); return got; }
        const PrimResult want = logged();
        note(got == want ? "primitives: CallNamedTree: the same natively (no tree)" : "primitives: CallNamedTree: DIFFERENT (the log's taken)");
        return want;
    }
    if (n.opcode == 32) {  // TestObjectType
        r.sync_variables(me, f, s);
        ScriptVariables vars(r.world, me, f);
        const auto t0 = Clock::now();
        PrimResult got = test_object_type(n, r.world, vars);
        if (r.bench) r.bench->other_us += us(t0, Clock::now());
        const PrimResult want = logged();
        if (got == want) note(std::string("primitives: TestObjectType: the same natively, ") + (got == PrimResult::True ? "true" : "false"));
        else {
            note("primitives: TestObjectType: DIFFERENT (the log's taken)");
            static int shown = 0;
            if (getenv("REPLAY_SHOW_DIFF") && shown++ < 8) {
                std::printf("TestObjectType at %d:%d:", s.tree, s.node);
                for (const LookupRec& v : s.lookups) std::printf(" [%d/%d=%d]", v.scope, v.data, v.value);
                std::printf(" native %d, log %d\n", (int)got, (int)want);
            }
            got = want;
        }
        return got;
    }
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
        std::string k = n.opcode == 2 ? "primitives: expression" : "primitives: SetToNext";
        if (n.opcode == 31) k += " kind " + std::to_string(n.operand[4] & 0x7F);
        if (got == want) note(k + ": the same natively");
        else {
            note(k + ": DIFFERENT (the log's taken)");
            static std::map<int, int> shown;
            const int kind = n.operand[4] & 0x7F;
            if (n.opcode == 31 && kind == 6 && getenv("REPLAY_KIND6")) {
                // which data fields of the object found equal the frame's first parameter
                if (const Object* x = r.world.find(s.stack_object_after); x && want == PrimResult::True && !s.params.empty()) {
                    static std::map<int, int> hits;
                    static int steps = 0;
                    steps++;
                    for (size_t k = 0; k < x->data.size(); k++) if (x->data[k] == s.params[0]) hits[(int)k]++;
                    if (steps % 200 == 0 || steps == 1) {
                        std::printf("kind 6 after %d true steps; fields equal to param 0:", steps);
                        for (auto [k, c] : hits) if (c > steps / 2) std::printf(" %d:%d", k, c);
                        std::printf("\n");
                    }
                }
            }
            if (n.opcode == 31 && getenv("REPLAY_SHOW_STN") && shown[kind]++ < 6) {
                std::printf("kind %d at %d:%d by %d operand", kind, s.tree, s.node, me.id());
                for (uint8_t b : n.operand) std::printf(" %02x", b);
                std::printf(": so %d -> %d, %s;", s.stack_object, s.stack_object_after, want == PrimResult::True ? "true" : "false");
                for (int16_t id : {s.stack_object, s.stack_object_after})
                    if (const Object* x = r.world.find(id)) std::printf(" [%d guid %08x person %d data 0..2 %d %d %d]", id, x->guid, x->person, x->data[0], x->data[1], x->data[2]);
                std::printf("\n");
            }
            got = want;
        }
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
    if (s.params != s.params_after || s.locals != s.locals_after || s.stack_object != s.stack_object_after)
        note("primitives: from the log, opcode " + std::to_string(n.opcode) + ": changes its frame");
    if (getenv("REPLAY_SHOW_OP") && n.opcode == (uint16_t)atoi(getenv("REPLAY_SHOW_OP"))) {
        static int shown = 0;
        if (shown++ < (getenv("REPLAY_SHOW_ALL") ? 100000 : 14)) {
            std::printf("op %d by %d at %d:%d (%s) operand", n.opcode, me.id(), s.tree, s.node, f.file ? f.file->path.c_str() : "?");
            for (uint8_t b : n.operand) std::printf(" %02x", b);
            std::printf(" -> %s; so %d; lookups", s.after == n.true_target ? "true" : "false", s.stack_object);
            for (const LookupRec& v : s.lookups) std::printf(" [%d/%d=%d]", v.scope, v.data, v.value);
            std::printf(" params");
            for (size_t k = 0; k < s.params.size(); k++) std::printf(" %d>%d", s.params[k], s.params_after[k]);
            std::printf(" locals");
            for (size_t k = 0; k < s.locals.size(); k++) std::printf(" %d>%d", s.locals[k], s.locals_after[k]);
            std::printf("\n");
        }
    }
    note("primitives: from the log, opcode " + std::to_string(n.opcode));
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
bool run(const Log& log, const Library& lib, const LotChunks& club, const std::vector<Neighbour>& neighbours, Bench* bench, bool card_lot = false) {
    World world(lib);
    world.load_lot(club);
    if (getenv("REPLAY_SHOW_OBJECTS"))
        for (const auto& o : world.objects())
{
            std::printf("object %d: guid %08x, %s; data", o->id(), o->guid, o->file ? o->file->path.c_str() : "-");
            for (int16_t v : o->data) std::printf(" %d", v);
            std::printf("\n");
        }
    if (const char* dump = getenv("REPLAY_DUMP_ARRAYS"))  // the lot's three arrays, raw, for study
        if (std::FILE* f = std::fopen(dump, "wb")) {
            for (const LotArray& la : read_lot_arrays(club)) std::fwrite(la.bytes.data(), 1, la.bytes.size(), f);
            std::fclose(f);
        }
    // The disc's club: its motives do not move by themselves (measured; World::motives_run).
    world.motives_run = card_lot;
    world.neighbours = neighbours;
    if (log.lists.empty() || log.lists[0].ids.size() != world.objects().size()) {
        std::printf("the log does not start with the lot loaded (%zu objects in its first list, %zu in the lot)\n",
                    log.lists.empty() ? (size_t)0 : log.lists[0].ids.size(), world.objects().size());
        return false;
    }

    Replay rp{lib, world};
    rp.card_lot = card_lot;
    rp.bench = bench;
    rp.log = &log;
    std::set<size_t> consumed;  // log steps a native primitive's checks have stepped
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
    std::set<uint32_t> first_step;                     // stacks whose first step was seen
    std::set<uint32_t> unseen;
    std::set<uint32_t> delivered;                      // Sims whose first interaction was seen                         // placeholder addresses of saved frames below a first step
    std::map<std::tuple<uint32_t, uint32_t, uint16_t>, std::pair<const Tree*, const ObjectFile*>> frame_tree;
    size_t streak = 0, best_streak = 0;

    // Which steps are turns: those in the tick's pass over the objects ('Q' records). The
    // others are the engine's own (the pie menu's checks on a Sim's stack: docs/simulation.md).
    // A log without them is taken as every step a turn.
    const bool have_passes = !log.passes.empty();
    std::vector<bool> engine_run(log.steps.size(), false);
    for (size_t i = 0; i < log.steps.size(); i++) engine_run[i] = have_passes && !log.steps[i].in_pass;
    if (!have_passes) note("log: no pass records (an older log): every step taken as a turn, no tick as paused");

    size_t place_at = 0;   // the next 'P' record (places: routing is not native, so set to the log's)
    size_t motive_at = 0;  // the next 'N' record
    int shown_motives = 0;
    for (uint32_t t = 1; t <= log.ticks; t++) {
        double tick_us = 0;
        for (; place_at < log.places.size() && log.places[place_at].tick < t; place_at++)
            if (Object* o = world.find(log.places[place_at].id)) {
                o->x16 = log.places[place_at].x;
                o->y16 = log.places[place_at].y;
                o->level = log.places[place_at].level;
            }
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
        std::vector<std::pair<Object*, bool>> motives_due;  // the persons reached, in id order, and whether their turn ends with the motive update
        size_t motives_done = 0;
        // Their updates, at the end of their turns: before the first step of an object with a
        // higher id (the pass goes up by id), or at the pass's end.
        auto motives_before = [&](int16_t id) {
            for (; motives_done < motives_due.size() && motives_due[motives_done].first->id() < id; motives_done++)
                sim.end_turn(*motives_due[motives_done].first, motives_due[motives_done].second);
        };
        const auto p0 = Clock::now();
        for (auto [o, asleep] : sim.pass_order()) {
            if (pass.stopped_at && o->id() > (int16_t)pass.stopped_at) break;
            if (o->person) motives_due.push_back({o, sim.begin_turn(*o)});
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
            if (consumed.count(i)) continue;
            if (auto id0 = ids.find(s.sim); id0 != ids.end()) motives_before(id0->second);
            {
                // A Sim waiting at IdleForInput with an entry queued, whose next logged steps are
                // the engine's checks on a frame above: the IdleForInput step (the log's next
                // 281:4 of this Sim) is run natively, its checks stepping through those steps.
                auto id0 = ids.find(s.sim);
                Object* w = id0 == ids.end() ? nullptr : world.find(id0->second);
                if (w && w->person && s.node == 0) {
                    // The evaluation (opcode 3 at the native top): its checks are this Sim's next
                    // logged steps up to the opcode-3 step itself (its tree and node, looked up).
                    Interpreter& ein = sim.stack_of(*w);
                    const auto& est = ein.stack();
                    const Node* en = !ein.done() && !est.empty() && est.back().tree && est.back().node < est.back().tree->nodes.size()
                                         ? &est.back().tree->nodes[est.back().node] : nullptr;
                    // Native where it is checked: the save's lot (Mom's house, all three
                    // evaluations exact); not the disc's club, whose evaluations look otherwise
                    // (docs/simulation.md, "Autonomy"). REPLAY_EVALUATION=0 or 1 overrides.
                    const char* ev = getenv("REPLAY_EVALUATION");
                    const bool evaluation = ev ? ev[0] == '1' : rp.card_lot;
                    if (evaluation && en && en->opcode == 3 && !(s.tree == (uint16_t)est.back().tree->id && s.node == est.back().node)) {
                        size_t k = i;
                        std::vector<size_t> nest;
                        for (size_t j : by_tick[t]) {
                            if (j < i || log.steps[j].sim != s.sim) continue;
                            // The opcode-3 step itself: its tree and node, in a frame below its checks'
                            // (a guard can have the same tree id in another file: the bed's 8233).
                            if (log.steps[j].tree == (uint16_t)est.back().tree->id && log.steps[j].node == est.back().node &&
                                log.steps[j].frame < s.frame) { k = j; break; }  // (its lookup is its last check's)
                            nest.push_back(j);
                        }
                        // The checks of an object the native world does not have (made during the
                        // session: the replay does not make objects natively yet) are set aside: from
                        // its check's start in the bottom frame to the next check's start.
                        {
                            const uint32_t bottom = log.steps[i].frame;
                            std::vector<size_t> kept;
                            bool skipping = false;
                            size_t skipped = 0;
                            for (size_t j : nest) {
                                const StepRec& ns = log.steps[j];
                                if (ns.frame == bottom && ns.node == 0) skipping = world.find(ns.stack_object) == nullptr;
                                if (skipping) { skipped++; consumed.insert(j); continue; }
                                kept.push_back(j);
                            }
                            if (skipped) note("autonomy: an evaluation's steps of objects not in the native world, set aside", skipped);
                            nest = kept;
                        }
                        if (k != i && nest.size() >= 20) {
                            rp.present.clear();
                            for (const ListRec& l : log.lists) if (l.tick <= t) rp.present = std::set<int16_t>(l.ids.begin(), l.ids.end());
                            rp.nested = nest;
                            rp.nested_pos = 0;
                            rp.step = &log.steps[k];
                            rp.file_of_top = est.back().file;
                            ein.reset_iterations();
                            ein.step();
                            note(rp.nested_pos == nest.size() ? "autonomy: the evaluation used exactly the log's steps of it" : "autonomy: the evaluation did NOT use the log's steps of it");
                            if (rp.nested_pos != nest.size() && getenv("REPLAY_SHOW_AUTONOMY"))
                                std::printf("evaluation of %d at tick %u: used %zu of %zu steps\n", w->id(), t, rp.nested_pos, nest.size());
                            for (size_t j : nest) consumed.insert(j);
                            consumed.insert(k);
                            rp.nested.clear();
                            continue;
                        }
                    }
                }
                if (w && w->person && !w->queue.empty() && s.node == 0) {
                    Interpreter& win = sim.stack_of(*w);
                    const auto& wst = win.stack();
                    const Node* wn = !win.done() && !wst.empty() && wst.back().tree && wst.back().node < wst.back().tree->nodes.size()
                                         ? &wst.back().tree->nodes[wst.back().node] : nullptr;
                    size_t k = i;
                    std::vector<size_t> nest;
                    for (size_t j : by_tick[t]) {
                        if (j < i || log.steps[j].sim != s.sim) continue;
                        if (log.steps[j].tree == 281 && log.steps[j].node == 4) { k = j; break; }
                        nest.push_back(j);
                    }
                    if (wn && wn->opcode == 17 && k != i) {
                        rp.nested = nest;
                        rp.nested_pos = 0;
                        rp.step = &log.steps[k];
                        rp.file_of_top = wst.back().file;
                        win.reset_iterations();
                        win.step();
                        note(rp.nested_pos == nest.size() ? "queue: the checks used exactly the log's steps of them" : "queue: the checks did NOT use the log's steps of them");
                        for (size_t j : nest) consumed.insert(j);
                        consumed.insert(k);
                        rp.nested.clear();
                        continue;
                    }
                }
            }
            auto& fr = frames[s.sim];
            auto at = std::find(fr.begin(), fr.end(), s.frame);
            if (at != fr.end()) fr.erase(at + 1, fr.end());
            else {
                // A frame not seen yet: a call (above the others), or a return into one of the
                // saved frames below the stack's first step, whose address the log had not shown
                // (placeholders in `unseen`): the frames above it are gone, and it takes the
                // place of the placeholder below them.
                bool popped = false;
                while (!fr.empty() && fr.back() > s.frame) { fr.pop_back(); popped = true; }
                if (popped && !fr.empty() && unseen.count(fr.back())) { unseen.erase(fr.back()); fr.back() = s.frame; }
                else fr.push_back(s.frame);
            }
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
            // Several files can hold a tree of the same id and shape (a Sim's template_person
            // and the character files): an own tree is taken from the object's file if it has it.
            if (tf.first && tree_kind(s.tree) == TreeKind::Own && o->file && tf.second != o->file && s.t && s.t->tree == s.tree) {
                auto ot = o->file->trees.find(s.tree);
                if (ot != o->file->trees.end() && same_shape(ot->second, s.t->raw)) tf = {&ot->second, o->file};
            }
            const auto [tree, file] = tf;
            if (!tree) {
                note("steps: tree not found on the disc");
                note(std::string("steps: tree not found: ") + (s.t ? "tree " : "no tree record, tree ") + std::to_string(s.tree));
                continue;
            }
            // The native stack before the step against the log's.
            const auto& st = in.stack();
            // The interaction queue: the first interaction pushed onto a waiting Sim (a tree at
            // node 0 above "Wait For Notify", Global 281, other than its checks) against the
            // first entry of the queue the lot saved for it.
            const StepRec* before = nullptr;
            for (size_t j = i; j-- > 0;) if (log.steps[j].sim == s.sim) { before = &log.steps[j]; break; }
            if (o->person && s.node == 0 && before && before->tree == 281 && before->node == 4 && delivered.insert(s.sim).second) {
                if (rp.natively_delivered.count(o->id())) {}  // checked by the native delivery
                else if (o->queue.empty()) note("queue: a waiting Sim's first interaction, with nothing queued (autonomy's)");
                else {
                    const SavedQueueEntry& q = o->queue.front();
                    const bool same = q.tree == (int16_t)s.tree && q.objects[1] == s.stack_object;
                    note(same ? "queue: a waiting Sim's first interaction the queue's first entry (tree, target)" : "queue: a waiting Sim's first interaction NOT its queue's first entry");
                    if (!same && getenv("REPLAY_SHOW_PUSH")) std::printf("queue: Sim %d got %d with %d; queued %d with %d\n", o->id(), s.tree, s.stack_object, q.tree, q.objects[1]);
                }
            }
            if (first_step.insert(s.sim).second) {
                // Each lot object's first step against the stack the lot saved.
                const bool same0 = !st.empty() && st.back().tree && st.back().tree->id == (int16_t)s.tree && st.back().node == s.node;
                note(same0 ? "load: a stack's first step at its saved top frame" : "load: a stack's first step NOT at its saved top frame");
                // The log shows only the frames that step: the saved frames below the first
                // step's are taken as the log's too (addresses below its, never seen).
                if (same0 && st.size() > fr.size()) {
                    std::vector<uint32_t> below;
                    for (size_t k = st.size() - fr.size(); k > 0; k--) { below.push_back(s.frame - 16 * (uint32_t)k); unseen.insert(below.back()); }
                    fr.insert(fr.begin(), below.begin(), below.end());
                }
                static int shown = 0;
                if (!same0 && getenv("REPLAY_SHOW_LOAD") && shown++ < 12)
                    std::printf("load: object %d tick %u: saved %d:%d (depth %zu), first step %d:%d (depth %zu)\n", o->id(), t,
                                st.empty() || !st.back().tree ? -1 : st.back().tree->id, st.empty() ? -1 : st.back().node, st.size(), s.tree, s.node, fr.size());
            }
            const bool same = !in.done() && st.size() == fr.size() && st.back().tree && st.back().tree->id == (int16_t)s.tree && st.back().node == s.node;
            if (same) {
                note("steps: the native stack as the log's");
                best_streak = std::max(best_streak, ++streak);
            } else {
                streak = 0;
                {
                    static std::map<int, int> shown;
                    const int cat = in.done() ? 0 : st.size() > fr.size() ? 1 : st.size() < fr.size() ? 2 : (st.back().tree && st.back().tree->id != (int16_t)s.tree) ? 3 : 4;
                    static const char* names[] = {"stopped", "deeper", "log deeper", "another tree", "another node"};
                    if (getenv("REPLAY_SHOW_DIFFS") && shown[cat]++ < 6) {
                        const StepRec* prev = nullptr;
                        for (size_t j = i; j-- > 0;) if (log.steps[j].sim == s.sim) { prev = &log.steps[j]; break; }
                        std::printf("%s: object %d tick %u: native %d:%d depth %zu (error %d), log %d:%d depth %zu; its last step %d:%d->%d %c res %d\n",
                                    names[cat], o->id(), t, st.empty() || !st.back().tree ? -1 : st.back().tree->id, st.empty() ? -1 : st.back().node,
                                    st.size(), (int)in.error(), s.tree, s.node, fr.size(), prev ? prev->tree : -1, prev ? prev->node : -1, prev ? prev->after : -1,
                                    prev && prev->look ? prev->look : '-', prev ? prev->result : -1);
                    }
                }
                if (in.done()) note("steps: different: the native stack stopped");
                else if (st.size() != fr.size()) note(st.size() < fr.size() ? "steps: different: the log's is deeper (a frame pushed)" : "steps: different: the native is deeper");
                if (!in.done() && st.size() < fr.size() && o->person && getenv("REPLAY_SHOW_PUSH")) {
                    const Frame& w = st.back();
                    const Node* wn = w.tree && w.node < w.tree->nodes.size() ? &w.tree->nodes[w.node] : nullptr;
                    std::printf("push: Sim %d tick %u onto %d:%d (op %d): %d:%d in %s, stack object %d, params", o->id(), t, w.tree ? w.tree->id : -1, w.node,
                                wn ? wn->opcode : -1, s.tree, s.node, file ? file->path.c_str() : "?", s.stack_object);
                    for (int16_t p : s.params) std::printf(" %d", p);
                    std::printf("; saved current id %d objects %d %d %d; queue", o->current.id, o->current.objects[0], o->current.objects[1], o->current.objects[2]);
                    for (const auto& q : o->queue) std::printf(" [id %d objects %d %d %d shorts %d %d %d %d a %d b %d c %d tree %d]", q.id, q.objects[0], q.objects[1], q.objects[2], q.shorts[0], q.shorts[1], q.shorts[2], q.shorts[3], q.a, q.b, q.c, q.tree);
                    std::printf("\n");
                }
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
                // A frame's file is the object file its trees are named from: for a global or
                // semi-global tree, not the file the tree is in but one that names it (the
                // object's own if it does, else any whose semi-global file it is).
                top.file = file;
                if (tree_kind(s.tree) == TreeKind::Global) top.file = !st.empty() && st.back().file ? st.back().file : o->file;
                else if (tree_kind(s.tree) == TreeKind::SemiGlobal) {
                    if (o->file && o->file->semi == file) top.file = o->file;
                    else for (const auto& f : lib.files()) if (f->semi == file) { top.file = f.get(); break; }
                }
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
        motives_before(INT16_MAX);
        if (have_passes) note(sim.stop_asked() == (pass.stopped_at != 0) ? "passes: stopped (or not) as the log's" : "passes: DIFFERENT stop");
        sim.end_pass(have_passes ? (int16_t)pass.stopped_at : 0);
        // The persons' motives after the tick against the log's ('N' records), then set to
        // the log's (what the scripts not native yet wrote).
        while (motive_at < log.motives.size() && log.motives[motive_at].tick < t) motive_at++;
        for (; motive_at < log.motives.size() && log.motives[motive_at].tick == t; motive_at++) {
            const MotiveRec& m = log.motives[motive_at];
            Object* o = world.find(m.id);
            if (!o || o->motives.size() < 16) { note("motives: a person not in the world"); continue; }
            bool same = true;
            for (int i = 0; i < 16; i++) if (i != 3 && i != 13 && o->motives[(size_t)i] != m.motives[(size_t)i]) same = false;
            note(same ? "motives: the native motives as the log's after the tick (all but 3 and 13)" : "motives: DIFFERENT after the tick (set to the log's)");
            if (!same)
                for (int i = 0; i < 16; i++)
                    if (i != 3 && i != 13 && o->motives[(size_t)i] != m.motives[(size_t)i])
                        note("motives: different after the tick: person " + std::to_string(m.id) + " motive " + std::to_string(i) +
                             (std::fabs(o->motives[(size_t)i] - m.motives[(size_t)i]) < 1e-3f ? " (by a last bit)" : ""));
            if (!same && getenv("REPLAY_SHOW_MOTIVES") && m.id == 237 && t >= 5020 && shown_motives++ < 6) {
                std::printf("tick %u person %d (object %p):", t, m.id, (void*)o);
                for (int i : {5, 6, 7, 8, 9, 14, 15}) std::printf("  m%d native %.6g log %.6g", i, o->motives[(size_t)i], m.motives[(size_t)i]);
                std::printf("\n");
            }
            std::copy(m.motives.begin(), m.motives.end(), o->motives.begin());
        }
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

    // Autonomy's evaluations: a person's many guard checks in one tick (each at node 0 in the
    // bottom frame of its checks), against the native candidates (autonomy_candidates), the
    // persons' interactions and a version-11 table's first tree set aside.
    if (!bench) {
        std::map<std::pair<uint32_t, uint32_t>, std::vector<const StepRec*>> starts;  // (tick, stack) -> node-0 starts
        for (const StepRec& s : log.steps)
            if (s.node == 0) starts[{s.tick, s.sim}].push_back(&s);
        for (auto& [key, v] : starts) {
            if (v.size() < 40) continue;
            std::map<uint32_t, size_t> per_frame;  // the checks' bottom frame: the one with the most starts
            for (const StepRec* s : v) per_frame[s->frame]++;
            const uint32_t bottom = std::max_element(per_frame.begin(), per_frame.end(), [](const auto& a, const auto& b) { return a.second < b.second; })->first;
            auto id = ids.find(key.second);
            const Object* me = id == ids.end() ? nullptr : world.find(id->second);
            if (!me || !me->person) continue;
            // An evaluation goes over the lot: many objects (a pie menu's checks are of one).
            std::set<int16_t> over;
            for (const StepRec* s : v) if (s->frame == bottom) over.insert(s->stack_object);
            if (over.size() < 10 || per_frame[bottom] < 20) continue;
            std::vector<std::pair<int16_t, uint16_t>> logged, native;
            for (const StepRec* s : v) {
                const Object* so = world.find(s->stack_object);
                if (s->frame != bottom || !so || so == me) continue;
                bool prefix = false;
                const ObjectFile* tf = so->person && so->file ? so->file->semi : so->file;
                if (tf) for (const auto& [a, p] : tf->prefix_of) if (p == s->tree) prefix = true;
                if (!prefix) logged.push_back({s->stack_object, s->tree});
            }
            // The objects on the lot then (the log's list at that tick: the replay does not
            // remove objects natively yet, as the paper girl leaves).
            std::set<int16_t> present;
            for (const ListRec& l : log.lists)
                if (l.tick <= key.first) present = std::set<int16_t>(l.ids.begin(), l.ids.end());
            for (const Candidate& c : autonomy_candidates(world, *me))
                if (present.empty() || present.count(c.object->id())) native.push_back({c.object->id(), c.interaction->guard});
            note(logged == native ? "autonomy: an evaluation's candidates the same natively (objects and persons)" : "autonomy: an evaluation's candidates DIFFERENT");
            note("autonomy: candidates checked in evaluations", logged.size());
            if (logged != native && getenv("REPLAY_SHOW_AUTONOMY")) {
                std::set<int16_t> lo, na;
                for (auto& x : logged) lo.insert(x.first);
                for (auto& x : native) na.insert(x.first);
                std::printf("objects only native:");
                for (int16_t i : na) if (!lo.count(i)) {
                    const Object* o = world.find(i);
                    std::printf(" %d(%s, stack %zu)", i, o && o->file ? o->file->path.substr(o->file->path.rfind('/') + 1).c_str() : "?", o ? o->stack.size() : 0);
                }
                std::printf("\nonly logged:");
                for (int16_t i : lo) if (!na.count(i)) std::printf(" %d", i);
                std::printf("\n");
                size_t k = 0;
                while (k < logged.size() && k < native.size() && logged[k] == native[k]) k++;
                std::printf("evaluation at tick %u by %d: %zu logged, %zu native, first difference at %zu: log %d/%d, native %d/%d\n", key.first, me->id(), logged.size(), native.size(), k,
                            k < logged.size() ? logged[k].first : -1, k < logged.size() ? logged[k].second : -1, k < native.size() ? native[k].first : -1, k < native.size() ? native[k].second : -1);
            }
        }
    }

    // Autonomy's scores ('A', 'B' records): each recomputed natively (interaction_score, the
    // curves from STR# 501) from the motives and adverts it read; the person is the one whose
    // evaluation it was (the Sim of that tick's evaluation: Sim 12's or Sim 237's motives).
    if (!bench && !log.scores.empty()) {
        const auto curves = score_curves(lib);
        note("autonomy: score curves from the global file", curves.size());
        for (const BaseRec& br : log.bases) {
            const ScoreRec* sr = nullptr;
            for (const ScoreRec& x : log.scores) if (x.tick == br.tick) { sr = &x; break; }
            if (!sr) continue;
            const Object* any = nullptr;
            for (const auto& o : world.objects()) if (o->person && !o->definition) { any = o.get(); break; }
            if (!any) continue;
            const std::vector<float> mv(sr->motives.begin(), sr->motives.end());
            const float got = interaction_score(curves, nullptr, *any, mv);
            note(got == br.score ? "autonomy: a base score the same natively (no advert)" : "autonomy: a base score DIFFERENT");
        }
        for (const ScoreRec& sr : log.scores) {
            // The person: the Sim whose motives these are (person data for the personality).
            const Object* person = nullptr;
            for (const auto& o : world.objects())
                if (o->person && !o->definition && o->motives.size() >= 16) {
                    bool near = true;
                    for (int i : {5, 7, 9}) if (std::fabs(o->motives[(size_t)i] - sr.motives[(size_t)i]) > 60.0f) near = false;
                    if (near && (!person || std::fabs(o->motives[5] - sr.motives[5]) < std::fabs(person->motives[5] - sr.motives[5]))) person = o.get();
                }
            if (!person) { note("autonomy: a score's person not found"); continue; }
            std::vector<Interaction::Advert> ads(16);
            for (int m = 0; m < 16; m++) ads[(size_t)m] = {sr.adverts[(size_t)m][1], sr.adverts[(size_t)m][2], (uint16_t)sr.adverts[(size_t)m][0]};
            const std::vector<float> mv(sr.motives.begin(), sr.motives.end());
            const float got = interaction_score(curves, &ads, *person, mv);
            note(got == sr.score ? "autonomy: a score the same natively (bit for bit)" : std::fabs(got - sr.score) < 1e-5f ? "autonomy: a score the same natively but for the last bits" : "autonomy: a score DIFFERENT");
        }
    }

    // Routing: each Sim's logged step from one tile to the next ('P'), against the lot's walls
    // and the doors' openings (Walls). Measured on the save's lot.
    if (!bench && !log.places.empty()) {
        note("routing: the lot's wall edges (each seen from both tiles)", world.walls.wall_edges());
        note("routing: the doors' openings", world.walls.openings());
        note("routing: the rooms (the regions the walls close)", world.walls.rooms());
        if (getenv("REPLAY_SHOW_ROUTING"))
            for (const Walls::Door& d : world.walls.doors()) std::printf("door %d,%d +%d,%d: room %u to %u\n", d.x, d.y, d.dx, d.dy, d.from, d.to);
        std::map<int16_t, std::pair<int, int>> tile;
        for (const PlaceRec& p : log.places) {
            const Object* o = world.find(p.id);
            if (!o || !o->person || p.x < 0 || p.y < 0) continue;
            const int x = p.x >> 4, y = p.y >> 4;
            auto it = tile.find(p.id);
            if (it != tile.end() && it->second != std::make_pair(x, y)) {
                const int dx = x - it->second.first, dy = y - it->second.second;
                if (std::abs(dx) + std::abs(dy) != 1) {
                    note("routing: a Sim's move not to a neighbouring tile (not a step)");
                    if (getenv("REPLAY_SHOW_ROUTING")) std::printf("Sim %d moved %d,%d -> %d,%d (tick %u)\n", p.id, it->second.first, it->second.second, x, y, p.tick);
                }
                else {
                    const bool ok = world.walls.can_step(it->second.first, it->second.second, dx, dy);
                    note(ok ? "routing: a Sim's step between tiles open natively" : "routing: a Sim's step through a WALL natively");
                    if (ok && world.walls.wall_between(it->second.first, it->second.second, dx, dy)) note("routing: a Sim's step through a door's opening");
                    if (!ok && getenv("REPLAY_SHOW_ROUTING")) std::printf("Sim %d through a wall: %d,%d -> %d,%d (tick %u)\n", p.id, it->second.first, it->second.second, x, y, p.tick);
                }
            }
            tile[p.id] = {x, y};
        }
        // The original's own planned routes ('G': waypoints in sixteenths): every tile change
        // along their lines open natively (a corner crossed: either way round), and the objects
        // whose tiles they cross.
        std::map<std::string, size_t> crossed;
        for (const PlanRec& pl : log.plans) {
            if (pl.points.size() < 2) continue;
            bool open = true;
            for (size_t i = 1; i < pl.points.size() && open; i++) {
                const auto [x0, y0] = pl.points[i - 1];
                const auto [x1, y1] = pl.points[i];
                const int n = std::max(std::abs(x1 - x0), std::abs(y1 - y0));
                int tx = x0 >> 4, ty = y0 >> 4;
                for (int k = 1; k <= n && open; k++) {
                    const int nx = (x0 + (x1 - x0) * k / n) >> 4, ny = (y0 + (y1 - y0) * k / n) >> 4;
                    if (nx == tx && ny == ty) continue;
                    const int dx = nx - tx, dy = ny - ty;
                    if (std::abs(dx) + std::abs(dy) == 1) open = world.walls.can_step(tx, ty, dx, dy);
                    else open = (world.walls.can_step(tx, ty, dx, 0) && world.walls.can_step(tx + dx, ty, 0, dy)) ||
                                (world.walls.can_step(tx, ty, 0, dy) && world.walls.can_step(tx, ty + dy, dx, 0));
                    if (!open && getenv("REPLAY_SHOW_ROUTING")) std::printf("plan at tick %u through a wall: %d,%d -> %d,%d\n", pl.tick, tx, ty, nx, ny);
                    tx = nx;
                    ty = ny;
                    for (const auto& o : world.objects())
                        if (!o->person && o->level == 1 && o->on_lot() && (o->x16 >> 4) == nx && (o->y16 >> 4) == ny && o->file)
                            crossed[o->file->path.substr(o->file->path.rfind('/') + 1)]++;
                }
            }
            note(open ? "routing: an original's planned route open natively (walls, doors)" : "routing: an original's planned route through a WALL natively");
            // The rooms along it (Walls::room): each change of room between two waypoints a
            // single step through a door's opening (the route broken at its doors), each piece
            // between within one room (its waypoints' tiles; a diagonal wall's tile, 0, either).
            bool at_doors = true;
            size_t changes = 0;
            for (size_t i = 1; i < pl.points.size(); i++) {
                const int x0 = pl.points[i - 1].first >> 4, y0 = pl.points[i - 1].second >> 4;
                const int x1 = pl.points[i].first >> 4, y1 = pl.points[i].second >> 4;
                const uint16_t r0 = world.walls.room(x0, y0), r1 = world.walls.room(x1, y1);
                if (r0 == r1 || !r0 || !r1) continue;
                changes++;
                const int dx = x1 - x0, dy = y1 - y0;
                bool door = false;
                for (const Walls::Door& d : world.walls.doors())
                    door |= (d.x == x0 && d.y == y0 && d.dx == dx && d.dy == dy) || (d.x == x1 && d.y == y1 && d.dx == -dx && d.dy == -dy);
                if (!door) {
                    at_doors = false;
                    if (getenv("REPLAY_SHOW_ROUTING")) std::printf("plan at tick %u changes room %u -> %u not at a door: %d,%d -> %d,%d\n", pl.tick, r0, r1, x0, y0, x1, y1);
                }
                else note("routing: a plan's change of room at a door, a waypoint either side");
            }
            if (changes) note(at_doors ? "routing: a plan through rooms broken at each door" : "routing: a plan changing rooms NOT at a door");
            else note("routing: a plan within one room");
        }
        if (getenv("REPLAY_SHOW_ROUTING"))
            for (const auto& [name, n] : crossed) std::printf("planned routes cross a tile of %s: %zu\n", name.c_str(), n);
        // Each walk (a Sim's tile changes, a pause of 40 ticks or more ending one) against the
        // native route between its ends (Walls::route, no object blocking yet).
        std::map<int16_t, std::vector<std::pair<uint32_t, Walls::Tile>>> seq;
        for (const PlaceRec& p : log.places) {
            const Object* o = world.find(p.id);
            if (!o || !o->person || p.x < 0 || p.y < 0) continue;
            const Walls::Tile t{p.x >> 4, p.y >> 4};
            auto& v = seq[p.id];
            if (v.empty() || v.back().second != t) v.push_back({p.tick, t});
        }
        for (auto& [id, v] : seq) {
            size_t start = 0;
            for (size_t i = 1; i <= v.size(); i++) {
                if (i < v.size() && v[i].first - v[i - 1].first < 40) continue;
                // A walk: v[start - 1 or start] .. v[i - 1] (from the tile it stood on).
                const size_t from = start > 0 ? start - 1 : start;
                if (i - 1 > from) {
                    std::vector<Walls::Tile> logged;
                    for (size_t k = from; k < i; k++) logged.push_back(v[k].second);
                    bool jump = false;
                    for (size_t k = 1; k < logged.size(); k++)
                        jump |= std::abs(logged[k].first - logged[k - 1].first) + std::abs(logged[k].second - logged[k - 1].second) != 1;
                    if (!jump) {
                        const auto r = world.walls.route(logged.front(), logged.back());
                        const bool same_length = r.size() == logged.size();
                        note(same_length ? "routing: a walk as long as the native route (walls only)" : "routing: a walk NOT as long as the native route (walls only)");
                        if (r == logged) note("routing: a walk's tiles the native route's");
                        if (!same_length && getenv("REPLAY_SHOW_ROUTING")) { for (auto& t : r) std::printf(" %d,%d", t.first, t.second); std::printf("\n"); }
                        if (getenv("REPLAY_SHOW_ROUTING"))
                            std::printf("walk of %d from tick %u: %zu steps, native %zu (%d,%d -> %d,%d)\n", id, v[from].first, logged.size() - 1,
                                        r.empty() ? (size_t)0 : r.size() - 1, logged.front().first, logged.front().second, logged.back().first, logged.back().second);
                    }
                }
                start = i;
            }
        }
    }

    // The motives' own update (update_motives), on each person's logged motives ('N'
    // records): the native update of one record against the next. A pair it does not
    // reproduce is a change the scripts made, or a tick the original did not update it.
    if (getenv("REPLAY_SHOW_TUNING") && lib.global())
        for (const auto& [id, t] : lib.global()->tuning)
            for (const auto& [name, v] : t) std::printf("tuning %u: %s = %g\n", id, name.c_str(), v);
    if (!bench && !log.motives.empty()) {
        MotiveRates rates;
        if (!rates.from(lib)) note("motives: a rate missing from the tuning tables");
        std::map<int16_t, std::vector<const MotiveRec*>> by_person;
        for (const MotiveRec& m : log.motives) by_person[m.id].push_back(&m);
        for (auto& [id, recs] : by_person) {
            const Object* o = world.find(id);
            if (!o || !o->person || o->person_data.size() < 7 || o->motives_b.size() < 16) continue;
            std::array<float, 16> last;
            std::copy_n(o->motives_b.begin(), 16, last.begin());
            for (size_t k = 1; k < recs.size(); k++) {
                std::array<float, 16> now = recs[k - 1]->motives, copy = last;
                update_motives(rates, now, copy, o->person_data[3], o->person_data[6], 12);
                bool same = true;
                for (int i = 0; i < 16; i++) if (i != 3 && i != 13 && now[(size_t)i] != recs[k]->motives[(size_t)i]) same = false;
                if (!same && getenv("REPLAY_SHOW_MOTIVES")) {
                    int near = 0;
                    for (int i = 0; i < 16; i++) if (i != 3 && i != 13 && std::fabs(now[(size_t)i] - recs[k]->motives[(size_t)i]) < 1e-3f) near++;
                    if (near == 14)
                        for (int i = 0; i < 16; i++)
                            if (i != 3 && i != 13 && now[(size_t)i] != recs[k]->motives[(size_t)i])
                                std::printf("motive %d of %d at tick %u: native %.9g, log %.9g (before %.9g)\n", i, id, recs[k]->tick, now[(size_t)i], recs[k]->motives[(size_t)i], recs[k - 1]->motives[(size_t)i]);
                }
                note(same ? "motives: an update the same natively (bit for bit, all but 3 and 13)" : "motives: a change not the update's (a script's, or no update)");
                // An update step (energy moves only by the update): the copy is the motives after it.
                if (now[kEnergy] == recs[k]->motives[kEnergy] && recs[k]->motives[kEnergy] != recs[k - 1]->motives[kEnergy]) last = recs[k]->motives;
                if (!same && now[kEnergy] == recs[k]->motives[kEnergy] && recs[k]->motives[kEnergy] != recs[k - 1]->motives[kEnergy])
                    note("motives: an update step with a script's change besides");
            }
        }
    }
    return true;
}

int main(int argc, char** argv) {
    if (argc < 3) { std::printf("usage: replay LOG ISO [--card CARD] [--lot N] [--bench]\n"); return 2; }
    Log log;
    if (!read_log(argv[1], log)) { std::printf("cannot read %s\n", argv[1]); return 1; }
    Disc d(argv[2]);
    if (!d.ok()) { std::printf("cannot read %s\n", argv[2]); return 1; }
    const Library lib = Library::bustin_out(d);
    bool bench = false;
    const char* card_path = nullptr;
    int lot_index = -1;
    for (int i = 3; i < argc; i++) {
        const std::string a = argv[i];
        if (a == "--bench") bench = true;
        else if (a == "--card" && i + 1 < argc) card_path = argv[++i];
        else if (a == "--lot" && i + 1 < argc) lot_index = std::atoi(argv[++i]);
        else { std::printf("unknown option %s\n", argv[i]); return 2; }
    }
    std::vector<uint8_t> ngh;
    if (card_path) {
        std::vector<uint8_t> card;
        if (std::FILE* f = std::fopen(card_path, "rb")) {
            card.resize(1 << 22);
            card.resize(std::fread(card.data(), 1, card.size(), f));
            std::fclose(f);
        }
        ngh = memory_card_file(card, "G4ME69");
        if (ngh.empty()) { std::printf("no Bustin' Out save on %s\n", card_path); return 1; }
        if (lot_index < 0) lot_index = 0;
    } else {
        ngh = d.read("Runtime/default.ngh");
    }
    const auto lots = read_lots(ngh);
    const auto neighbours = read_neighbours(ngh);
    if (lots.empty()) { std::printf("no lots\n"); return 1; }
    const LotChunks& the_lot = lot_index < 0 || lot_index >= (int)lots.size() ? lots.back() : lots[(size_t)lot_index];
    if (!bench) {
        if (!run(log, lib, the_lot, neighbours, nullptr, card_path != nullptr)) return 1;
        for (const auto& [k, n] : counts) std::printf("  %-66s %zu\n", k.c_str(), n);
        return 0;
    }
    // The benchmark: five runs, each figure the median of the runs (the PC is shared).
    quiet = true;
    std::vector<Bench> runs(5);
    for (Bench& b : runs) if (!run(log, lib, the_lot, neighbours, &b, card_path != nullptr)) return 1;
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
