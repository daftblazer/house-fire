#include "housefire/world.h"

#include <algorithm>

namespace housefire {

Object& World::add(std::unique_ptr<Object> o, bool front) {
    Object& r = *o;
    if (front) objects_.insert(objects_.begin(), std::move(o));
    else objects_.push_back(std::move(o));
    reindex(r);
    return r;
}

LotObjects World::read_objects(const LotChunks& lot) const {
    const std::vector<uint8_t>* objt = lot.find("objt");
    const std::vector<uint8_t>* objm = lot.find("ObjM");
    if (!objt || !objm) throw FormatError("a lot without objt or ObjM");
    std::map<int, uint32_t> guid_of;  // objt field 4 -> GUID
    for (const ObjectType& t : read_object_types(*objt)) guid_of[t.fields[4]] = t.guid;
    return read_lot_objects(*objm, [&](int16_t type) {
        auto t = guid_of.find(type);
        return saved_class(t == guid_of.end() ? nullptr : lib_.definition(t->second));
    });
}

int World::load_lot(const LotChunks& lot) {
    const std::vector<ObjectType> types = read_object_types(*lot.find("objt"));
    std::map<int, const ObjectType*> by_number;  // objt field 4
    for (const ObjectType& t : types) by_number[t.fields[4]] = &t;
    int missing = 0;
    for (const LotObject& lo : read_objects(lot).objects) {
        auto o = std::make_unique<Object>();
        auto t = by_number.find(lo.type);
        if (t != by_number.end()) {
            o->guid = t->second->guid;
            o->definition = lib_.definition(o->guid, &o->file);
        }
        // No definition on the disc: a character, whose definition is made at run time.
        // Its scripts are the template person's, which every Sim runs (its own trees, 4096 on;
        // the character files hold only its data).
        if (!o->definition) {
            missing++;
            o->person = true;
            o->file = lib_.find("template_person");
        }
        // Its state as the lot saved it.
        o->data = lo.data;
        o->attributes = lo.attributes;
        for (int k = 0; k < kObjectTemps && k < (int)lo.temps.size(); k++) o->temps[(size_t)k] = lo.temps[(size_t)k];
        if (lo.saved_class == SavedClass::Part || lo.saved_class == SavedClass::Door)
            o->leader = lo.leader ? lo.leader : lo.id;
        if (lo.saved_class == SavedClass::Sim) {
            o->person_data = lo.person_data;
            o->motives = lo.motives;
        }
        for (const SavedFrame& sf : lo.stack) {
            Frame f;
            f.tree = lib_.tree(o->file, sf.tree);
            f.node = sf.node;
            f.params = sf.params;
            f.locals = sf.locals;
            f.stack_object = sf.stack_object;
            f.file = o->file;
            f.engine = sf.engine;
            o->stack.push_back(std::move(f));
        }
        add(std::move(o));
    }
    return missing;
}

SavedClass World::saved_class(const Definition* d) {
    // OBJD field 8, the master id (a multi-tile object's parts have one); field 7, the type
    // (8 a door).
    if (!d) return SavedClass::Sim;
    if (d->field(8)) return d->field(7) == 8 ? SavedClass::Door : SavedClass::Part;
    return SavedClass::Object;
}

int16_t World::next_id() const {
    int16_t top = 0;
    for (const auto& o : objects_) top = std::max(top, o->id());
    return (int16_t)(top + 1);
}

Object& World::make_object(std::unique_ptr<Object> o) {
    if (o->data.size() <= kDataObjectId) o->data.resize(kDataObjectId + 1);
    o->data[kDataObjectId] = next_id();
    return add(std::move(o), true);
}

void World::remove(const Object* o) {
    for (auto it = by_id_.begin(); it != by_id_.end();)
        it = it->second == o ? by_id_.erase(it) : std::next(it);
    std::erase_if(objects_, [&](const std::unique_ptr<Object>& p) { return p.get() == o; });
    version_++;
}

Object* World::find(int16_t id) {
    auto it = by_id_.find(id);
    return it == by_id_.end() ? nullptr : it->second;
}

void World::reindex(Object& o) {
    for (auto it = by_id_.begin(); it != by_id_.end();)
        it = it->second == &o ? by_id_.erase(it) : std::next(it);
    if (o.id()) by_id_[o.id()] = &o;
    version_++;
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
        case Scope::CheckTreeAdRange: return at(world_.ad_range, d);
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
        case Scope::CheckTreeAdRange: return Cell{&world_.ad_range, d, false};
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
        case Scope::StackDefinition: {
            // The stack object's definition as the engine keeps it: a 32-bit version, then the
            // OBJD's fields, the datum counting 16-bit words from the start (so field datum - 2).
            const Object* so = stack_object();
            if (!so || !so->definition || d < 0) return std::nullopt;
            if (d < 2) return Variable{(int16_t)(d == 0 ? so->definition->version >> 16 : so->definition->version & 0xFFFF), {}};
            return Variable{(int16_t)so->definition->field(d - 2), {}};
        }
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
