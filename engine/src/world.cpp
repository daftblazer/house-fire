#include "housefire/world.h"

#include <algorithm>

namespace housefire {

Object& World::add(std::unique_ptr<Object> o) {
    Object& r = *o;
    objects_.push_back(std::move(o));
    reindex(r);
    return r;
}

void World::remove(const Object* o) {
    for (auto it = by_id_.begin(); it != by_id_.end();)
        it = it->second == o ? by_id_.erase(it) : std::next(it);
    std::erase_if(objects_, [&](const std::unique_ptr<Object>& p) { return p.get() == o; });
}

Object* World::find(int16_t id) {
    auto it = by_id_.find(id);
    return it == by_id_.end() ? nullptr : it->second;
}

void World::reindex(Object& o) {
    for (auto it = by_id_.begin(); it != by_id_.end();)
        it = it->second == &o ? by_id_.erase(it) : std::next(it);
    if (o.id()) by_id_[o.id()] = &o;
}

namespace {
// A table's entry, the table grown to reach it (the counts are each object's own; to be
// taken from its definition).
template <class T> T* at(std::vector<T>& v, int i) {
    if (i < 0 || i > 4096) return nullptr;
    if ((size_t)i >= v.size()) v.resize((size_t)i + 1);
    return &v[(size_t)i];
}
}  // namespace

Object* ScriptVariables::stack_object() { return world_.find(frame_.stack_object); }

int16_t* ScriptVariables::cell16(uint8_t scope, int16_t d) {
    switch ((Scope)scope) {
        case Scope::MyAttribute: return at(me_.attributes, d);
        case Scope::StackAttribute: { Object* s = stack_object(); return s ? at(s->attributes, d) : nullptr; }
        case Scope::MyData: return at(me_.data, d);
        case Scope::StackData: { Object* s = stack_object(); return s ? at(s->data, d) : nullptr; }
        case Scope::Global: return at(world_.globals, d);
        case Scope::Temp: return d >= 0 && d < 8 ? &me_.temps[(size_t)d] : nullptr;
        case Scope::Parameter: return d >= 0 && (size_t)d < frame_.params.size() ? &frame_.params[(size_t)d] : nullptr;
        case Scope::StackObject: return &frame_.stack_object;
        case Scope::MyPersonData: return me_.person ? at(me_.person_data, d) : nullptr;
        case Scope::StackPersonData: { Object* s = stack_object(); return s && s->person ? at(s->person_data, d) : nullptr; }
        case Scope::StackAttributeByParameter: {
            Object* s = stack_object();
            if (!s || d < 0 || (size_t)d >= frame_.params.size()) return nullptr;
            return at(s->attributes, frame_.params[(size_t)d]);
        }
        case Scope::Local: return d >= 0 && (size_t)d < frame_.locals.size() ? &frame_.locals[(size_t)d] : nullptr;
        case Scope::MyPersonDataByTemp:
            return me_.person && d >= 0 && d < 8 ? at(me_.person_data, me_.temps[(size_t)d]) : nullptr;
        case Scope::Neighbourhood: return at(world_.neighbourhood, d);
        default: return nullptr;
    }
}

float* ScriptVariables::cell_float(uint8_t scope, int16_t d) {
    switch ((Scope)scope) {
        case Scope::MyMotive: return me_.person ? at(me_.motives, d) : nullptr;
        case Scope::StackMotive: { Object* s = stack_object(); return s && s->person ? at(s->motives, d) : nullptr; }
        default: return nullptr;
    }
}

std::optional<ScriptVariables::Cell> ScriptVariables::cell(uint8_t scope, int16_t d) {
    // The tables by their owner, so that two scopes reaching the same storage give the same cell.
    auto owner = [&](Object* o, int which) -> const void* { return o ? (const char*)o + which : nullptr; };
    switch ((Scope)scope) {
        case Scope::MyAttribute: return Cell{owner(&me_, 0), d, false};
        case Scope::StackAttribute: return Cell{owner(stack_object(), 0), d, false};
        case Scope::MyData: return Cell{owner(&me_, 1), d, false};
        case Scope::StackData: return Cell{owner(stack_object(), 1), d, false};
        case Scope::MyPersonData: return Cell{owner(&me_, 2), d, false};
        case Scope::StackPersonData: return Cell{owner(stack_object(), 2), d, false};
        case Scope::MyMotive: return Cell{owner(&me_, 3), d, true};
        case Scope::StackMotive: return Cell{owner(stack_object(), 3), d, true};
        case Scope::Temp: return Cell{owner(&me_, 4), d, false};
        case Scope::Global: return Cell{&world_.globals, d, false};
        case Scope::Neighbourhood: return Cell{&world_.neighbourhood, d, false};
        case Scope::Parameter: return Cell{&frame_.params, d, false};
        case Scope::Local: return Cell{&frame_.locals, d, false};
        case Scope::StackObject: return Cell{&frame_.stack_object, 0, false};
        case Scope::StackAttributeByParameter:
            if (d < 0 || (size_t)d >= frame_.params.size()) return std::nullopt;
            return Cell{owner(stack_object(), 0), frame_.params[(size_t)d], false};
        case Scope::MyPersonDataByTemp:
            if (d < 0 || d >= 8) return std::nullopt;
            return Cell{owner(&me_, 2), me_.temps[(size_t)d], false};
        default: return std::nullopt;
    }
}

std::optional<Variable> ScriptVariables::get(uint8_t scope, int16_t d) {
    switch ((Scope)scope) {
        case Scope::Literal: return Variable{d, {}};
        case Scope::Constant: {
            // From the frame's file: the tree's, not necessarily the object's own (a Sim
            // running a fridge's interaction reads the fridge's constants).
            const std::optional<int16_t> c = world_.library().constant(frame_.file ? frame_.file : me_.file, d);
            return c ? std::optional<Variable>(Variable{*c, {}}) : std::nullopt;
        }
        default: break;
    }
    if (float* f = cell_float(scope, d)) return Variable{(int16_t)*f, *f};
    if (int16_t* p = cell16(scope, d)) return Variable{*p, {}};
    return std::nullopt;
}

bool ScriptVariables::set(uint8_t scope, int16_t d, int16_t value) {
    int16_t* p = cell16(scope, d);
    if (!p) return false;
    *p = value;
    if ((scope == (uint8_t)Scope::MyData && d == kDataObjectId)) world_.reindex(me_);
    if (scope == (uint8_t)Scope::StackData && d == kDataObjectId)
        if (Object* s = stack_object()) world_.reindex(*s);
    return true;
}

bool ScriptVariables::set_float(uint8_t scope, int16_t d, float value) {
    float* f = cell_float(scope, d);
    if (!f) return false;
    *f = value;
    return true;
}

}  // namespace housefire
