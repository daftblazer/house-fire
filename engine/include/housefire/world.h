// The objects scripts run on, and what a script's variables are (scope and datum): the
// object model. Its layout is the engine's own; what each scope names is the original's,
// measured on the recompiled Bustin' Out (docs/objects.md).
#pragma once
#include <array>
#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <vector>

#include "housefire/expression.h"
#include "housefire/interp.h"
#include "housefire/library.h"
#include "housefire/save.h"

namespace housefire {

// The scopes, by the PC game's names where the measurements agree with them.
enum class Scope : uint8_t {
    MyAttribute = 0, StackAttribute = 1, MyData = 3, StackData = 4, Global = 6, Literal = 7,
    Temp = 8, Parameter = 9, StackObject = 10, MyMotive = 14, StackMotive = 15,
    CheckTreeAdRange = 12, MyPersonData = 18, StackPersonData = 19, StackDefinition = 21, StackAttributeByParameter = 22, Local = 25,
    Constant = 26, MyPersonDataByTemp = 30, Neighbourhood = 34,
};

// Object data fields with a meaning the engine relies on.
enum : int { kDataObjectId = 11 };

struct Object {
    const ObjectFile* file = nullptr;
    const Definition* definition = nullptr;
    uint32_t guid = 0;
    std::array<int16_t, 8> temps{};   // scope 8
    std::vector<int16_t> data;        // scope 3, 72 fields (11: the object's id)
    std::vector<int16_t> attributes;  // scope 0, as many as the save gives (a lot's objects)
    bool person = false;
    std::vector<int16_t> person_data; // scope 18
    std::vector<float> motives;       // scope 14, kept between -100 and 100

    int16_t id() const { return data.size() > kDataObjectId ? data[kDataObjectId] : 0; }
};

class World {
public:
    explicit World(const Library& lib) : lib_(lib) {}
    const Library& library() const { return lib_; }

    // Objects are kept in the original's list order (the order SetToNext walks): a lot's in
    // its table's order, an object made later in front of them all.
    Object& add(std::unique_ptr<Object> o, bool front = false);
    const std::vector<std::unique_ptr<Object>>& objects() const { return objects_; }

    // A lot's objects, as its table lists them: each with its id, its definition (by its
    // type's GUID) and its file. Returns how many had no definition on the disc (the
    // characters, whose definitions are made at run time).
    int load_lot(const LotChunks& lot);
    // A lot unloaded: every object gone, the shared tables kept.
    void clear() { objects_.clear(); by_id_.clear(); }
    // An object made during play (CreateObject, the engine's effects): the next id (one more
    // than the highest in use: a removed object's id comes back), in front of the list.
    Object& make_object(std::unique_ptr<Object> o);
    int16_t next_id() const;
    void remove(const Object* o);
    Object* find(int16_t id);  // by object id (data 11)
    // Called when an object's id changes (its data 11 is written).
    void reindex(Object& o);

    std::vector<int16_t> globals;        // scope 6
    std::vector<int16_t> neighbourhood;  // scope 34
    std::vector<int16_t> ad_range;       // scope 12: one shared value per datum (only 15 is used)

private:
    const Library& lib_;
    std::vector<std::unique_ptr<Object>> objects_;
    std::map<int16_t, Object*> by_id_;
};

// The variables a primitive of `me` sees while running `frame`.
class ScriptVariables : public Variables {
public:
    ScriptVariables(World& w, Object& me, Frame& frame) : world_(w), me_(me), frame_(frame) {}

    std::optional<Variable> get(uint8_t scope, int16_t data) override;
    bool set(uint8_t scope, int16_t data, int16_t value) override;
    bool set_float(uint8_t scope, int16_t data, float value) override;

    // Where a variable is: its cell, for the scopes that are storage (nothing for literals,
    // constants and scopes not modelled yet). For checks against the original.
    struct Cell { const void* table; int index; bool is_float; };
    std::optional<Cell> cell(uint8_t scope, int16_t data);

private:
    Object* stack_object();
    int16_t* cell16(uint8_t scope, int16_t data);
    float* cell_float(uint8_t scope, int16_t data);

    World& world_;
    Object& me_;
    Frame& frame_;
};

}  // namespace housefire
