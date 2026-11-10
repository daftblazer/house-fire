#include "housefire/primitives.h"

namespace housefire {

namespace {
// A GUID in a node's operand, in the file's byte order (Bustin' Out's files are little-endian).
uint32_t operand_guid(const Node& n) {
    const auto& o = n.operand;
    return n.order == Order::Little ? (uint32_t)o[0] | o[1] << 8 | o[2] << 16 | (uint32_t)o[3] << 24
                                    : (uint32_t)o[0] << 24 | o[1] << 16 | o[2] << 8 | o[3];
}
}  // namespace

PrimResult random(const Node& n, World& world, ScriptVariables& vars) {
    const auto& o = n.operand;
    auto field = [&](int i) { return (int16_t)(n.order == Order::Little ? o[i] | o[i + 1] << 8 : o[i] << 8 | o[i + 1]); };
    const std::optional<Variable> range = vars.get(o[6], field(4));
    if (!range) return PrimResult::Stop;
    const int16_t v = range->value > 0 ? (int16_t)world.rng.below((uint32_t)range->value) : 0;
    return vars.set(o[2], field(0), v) ? PrimResult::True : PrimResult::Stop;
}

PrimResult call_named_tree(const Node& n, const Frame& f, const Library& lib) {
    const auto& o = n.operand;
    auto field = [&](int i) { return (int16_t)(n.order == Order::Little ? o[i] | o[i + 1] << 8 : o[i] << 8 | o[i + 1]); };
    const ObjectFile* own = f.file;
    const ObjectFile* file = nullptr;
    switch (field(2)) {
        case 1: file = lib.global(); break;
        case 2: file = own ? own->semi : nullptr; break;
        case 3: file = own; break;
        default:  // the calling tree's own kind
            if (f.tree) {
                const TreeKind k = tree_kind((uint16_t)f.tree->id);
                file = k == TreeKind::Global ? lib.global() : k == TreeKind::SemiGlobal ? (own ? own->semi : nullptr) : own;
            }
    }
    if (!file) return PrimResult::False;
    auto table = file->strings.find((uint16_t)field(0));
    if (table == file->strings.end() || o[4] >= table->second.size() || table->second[o[4]].empty()) return PrimResult::False;
    if (!file->tree_by_name.count(table->second[o[4]])) return PrimResult::False;
    return PrimResult::Stop;  // the call: not modelled yet
}

PrimResult idle_for_input(Interpreter& in, const Node& n, Object& me, World& world, Host& host) {
    (void)n;
    if (me.queue.empty()) return PrimResult::Stop;
    const SavedQueueEntry entry = me.queue.front();
    const Object* target = world.find(entry.objects[1]);
    const ObjectFile* global = world.library().global();
    if (!target || !global) return PrimResult::Stop;
    const Tree* action = world.library().tree(target->file, (uint16_t)entry.tree);
    if (!action || !target->file) return PrimResult::Stop;
    // The checks, each as the target's file names it: the tree its interaction table names to
    // run first (the club's: Global 463, "Dream Sequence - Prefix Check Tree"), then the
    // interaction's guard, where the table has them.
    for (const auto* table : {&target->file->prefix_of, &target->file->guard_of}) {
        auto c = table->find((uint16_t)entry.tree);
        if (c == table->end()) continue;
        const Tree* t = world.library().tree(target->file, c->second);
        const std::optional<bool> ok = t ? host.run_check(t, target->file, target->id()) : std::nullopt;
        if (!ok || !*ok) return PrimResult::Stop;
    }
    me.queue.erase(me.queue.begin());
    // The frame the primitive was given is not touched after this: the push can move it.
    if (!in.push(action, nullptr, 0)) return PrimResult::Stop;
    in.top().stack_object = target->id();
    in.top().file = target->file;
    return PrimResult::Continue;
}

PrimResult push_action(Interpreter& in, Frame& f, const Node& n, World& world) {
    Object* sim = world.find(f.stack_object);
    const Object* target = world.find(in.local(f, n.operand[1]));
    if (!sim || !target || !target->file || target->file->ttabs != 1) return PrimResult::Stop;
    const auto& table = target->file->interactions;
    if (n.operand[0] >= table.size()) return PrimResult::Stop;
    SavedQueueEntry e;
    e.objects[0] = sim->id();
    e.objects[1] = target->id();
    e.tree = (int16_t)table[n.operand[0]].first;
    sim->queue.push_back(e);
    return PrimResult::True;
}

PrimResult present(const Node& n, World& world, const Object& me) {
    world.presentation.push_back({n.opcode, me.id(), n.operand});
    return PrimResult::True;
}

PrimResult test_object_type(const Node& n, World& world, ScriptVariables& vars) {
    const auto& o = n.operand;
    const int16_t data = (int16_t)(n.order == Order::Little ? o[4] | o[5] << 8 : o[4] << 8 | o[5]);
    const std::optional<Variable> id = vars.get(o[6], data);
    if (!id) return PrimResult::Stop;
    const Object* x = world.find(id->value);
    return x && x->guid == operand_guid(n) ? PrimResult::True : PrimResult::False;
}

PrimResult set_to_next(const Node& n, World& world, ScriptVariables& vars) {
    const auto& o = n.operand;
    const uint8_t kind = o[4] & 0x7F;
    const uint8_t scope = (o[4] & 0x80) ? o[5] : (uint8_t)Scope::StackObject;
    const int16_t data = (o[4] & 0x80) ? (int16_t)o[7] : 0;
    const uint32_t guid = operand_guid(n);
    // Kind 6: an object whose data field 59 is the frame's first parameter (Castaway's
    // TrySetToNext, the same in Bustin' Out's log).
    const std::optional<Variable> p0 = kind == 6 ? vars.get((uint8_t)Scope::Parameter, 0) : std::nullopt;
    if (kind == 6 && !p0) return PrimResult::Stop;
    auto fits = [&](const Object& x) {
        switch (kind) {
            case 6: return x.data.size() > 59 && x.data[59] == p0->value;
            case 0: return true;
            case 1: return x.person;
            case 2: return !x.person;
            case 4: return x.guid == guid;
            default: return false;
        }
    };
    if (kind > 6) return PrimResult::Stop;
    const std::optional<Variable> cur = vars.get(scope, data);
    if (!cur) return PrimResult::Stop;
    if (kind == 5) {
        // The next neighbour by id; after the last, the target 0 and false.
        for (const Neighbour& nb : world.neighbours)
            if (nb.id > cur->value) return vars.set(scope, data, nb.id) ? PrimResult::True : PrimResult::Stop;
        vars.set(scope, data, 0);
        return PrimResult::False;
    }
    if (kind == 3) {
        // The next part of the target's multi-tile object, in the list's order, round from the
        // last part to the first; false if the target is not a part of one.
        const Object* t = world.find(cur->value);
        if (!t || !t->leader) return PrimResult::False;
        const auto& list = world.objects();
        size_t at = 0;
        while (at < list.size() && list[at].get() != t) at++;
        for (size_t k = 1; k < list.size(); k++) {
            const Object& x = *list[(at + k) % list.size()];
            if (x.leader == t->leader) return vars.set(scope, data, x.id()) ? PrimResult::True : PrimResult::Stop;
        }
        return PrimResult::False;
    }
    const auto& list = world.objects();
    size_t i = 0;
    if (cur->value != 0)
        for (size_t k = 0; k < list.size(); k++)
            if (list[k]->id() == cur->value) { i = k + 1; break; }
    for (; i < list.size(); i++)
        if (fits(*list[i])) return vars.set(scope, data, list[i]->id()) ? PrimResult::True : PrimResult::Stop;
    return PrimResult::False;
}

PrimResult idle(Interpreter& in, Frame& f, const Node& n) {
    const int16_t index = (int16_t)n.operand16(0);
    if (index < 0 || (size_t)index >= f.params.size()) return PrimResult::True;
    if (in.woke()) {
        in.wake();
        f.params[(size_t)index] = 0;
        return PrimResult::True;
    }
    const int16_t ticks = f.params[(size_t)index];
    if (ticks <= 0) return PrimResult::True;
    in.sleep((uint32_t)ticks - 1);
    return PrimResult::Yield;
}

}  // namespace housefire
