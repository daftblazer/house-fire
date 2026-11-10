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
#include "housefire/routing.h"
#include "housefire/save.h"

namespace housefire {

// The scopes, by the PC game's names where the measurements agree with them.
enum class Scope : uint8_t {
    MyAttribute = 0, StackAttribute = 1, MyData = 3, StackData = 4, Global = 6, Literal = 7,
    Temp = 8, Parameter = 9, StackObject = 10, MyMotive = 14, StackMotive = 15,
    CheckTreeAdRange = 12, MyPersonData = 18, StackPersonData = 19, StackDefinition = 21, StackAttributeByParameter = 22, Local = 25,
    NeighbourInStackObject = 24, Constant = 26, MyPersonDataByTemp = 30, Neighbourhood = 34,
};

// Object data fields with a meaning the engine relies on.
enum : int { kDataObjectId = 11 };
// Person data fields with a meaning the engine relies on: a Sim's neighbour id (its NBRS id).
enum : int { kPersonNeighbourId = 31 };

struct Object {
    const ObjectFile* file = nullptr;
    const Definition* definition = nullptr;
    uint32_t guid = 0;
    std::array<int16_t, 8> temps{};   // scope 8
    std::vector<int16_t> data;        // scope 3, 72 fields (11: the object's id)
    std::vector<int16_t> attributes;  // scope 0, as many as the save gives (a lot's objects)
    bool person = false;
    // A multi-tile object's parts: the id of its leader (the leader's own id for itself); 0
    // for a single-tile object.
    int16_t leader = 0;
    bool door = false;  // a door's part (OBJD type 8; World::saved_class)
    std::vector<int16_t> person_data; // scope 18
    std::vector<float> motives;       // scope 14, kept between -100 and 100
    std::vector<float> motives_b;
    // Its running motive changes (SetMotiveDelta's; the save's +1120 list): each a motive,
    // its change a tick and the value it stops at, applied as each of its turns begins.
    std::vector<LotObject::MotiveDelta> motive_deltas;
    uint32_t turns = 0;
    // Its place in sixteenths of a tile (the save's header 0 and 1, the original's FTilePt at
    // its +0xC8; the observer's 'P' records): -16, -16 off the lot.
    int32_t x16 = 0, y16 = 0, level = 1;
    bool on_lot() const { return x16 >= 0 && y16 >= 0; }               // the passes that have reached it (a person's motives update every 60th)     // the motives as of their last update (Motives::Sim's copy)
    // A Sim's interaction queue as the lot saved it: the entry it was on, the rest queued.
    SavedQueueEntry current;
    std::vector<SavedQueueEntry> queue;
    // Its tree stack, bottom first: a lot's objects' as the lot saved it (a frame whose tree
    // is not on the disc keeps a null tree).
    std::vector<Frame> stack;
    // Its relationships with other objects, as the lot saved them (the relationship system,
    // Relationship2, is not native yet).
    std::vector<SavedRelationship> relationships;

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
    Walls walls;  // the lot's (load_lot)
    // The class the original's loader makes of an object of this definition (none: a
    // character's): docs/lots.md, "The classes".
    static SavedClass saved_class(const Definition* d);
    // A lot's objects as saved, each read by its class.
    LotObjects read_objects(const LotChunks& lot) const;
    // A lot unloaded: every object gone, the shared tables kept.
    void clear() { objects_.clear(); by_id_.clear(); version_++; }
    // An object made during play (CreateObject, the engine's effects): the next id (one more
    // than the highest in use: a removed object's id comes back), in front of the list.
    Object& make_object(std::unique_ptr<Object> o);
    int16_t next_id() const;
    void remove(const Object* o);
    Object* find(int16_t id);  // by object id (data 11)
    // Called when an object's id changes (its data 11 is written).
    void reindex(Object& o);
    // The objects by id, and a count that changes whenever an object comes, goes or changes
    // its id (for those that keep their own order of the objects, as the simulator does).
    const std::map<int16_t, Object*>& by_id() const { return by_id_; }
    uint64_t version() const { return version_; }

    // The game's random numbers (Random): the engine's own generator, not the original's
    // (docs/plan.md, "Same game, new engine"). Seeded per game; xorshift64*.
    struct Rng {
        uint64_t state = 0x9E3779B97F4A7C15ull;
        uint32_t next() {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            return (uint32_t)((state * 0x2545F4914F6CDD1Dull) >> 32);
        }
        // A number from 0 up to n (n above 0), without the bias of a plain modulo.
        uint32_t below(uint32_t n) { return (uint32_t)(((uint64_t)next() * n) >> 32); }
    } rng;
    // What the simulation asks of the presentation (sound, the picture), in the order asked:
    // the front ends (not written yet) take them each frame. The simulation never waits on
    // them, so its run is the same with or without a front end.
    struct PresentationEvent {
        uint16_t opcode;                 // the primitive: PlaySound 23, Update 7, KillSounds 48
        int16_t object;                  // the object whose script asked
        std::array<uint8_t, 8> operand;  // as the node has it
    };
    std::vector<PresentationEvent> presentation;
    // A dialog waiting for the player (Dialog): the object whose script opened it (0: none),
    // and the player's answer once given (the front end sets it). While it waits, the tick's
    // pass stops after that object's turn (stop_pass, which the simulator takes and clears).
    int16_t dialog_object = 0;
    std::optional<bool> dialog_answer;
    bool stop_pass = false;
    // The clock's hour (the motive update reads it only for a sleeping person; the clock is
    // not modelled yet).
    int hour = 12;
    // Whether the persons' motives move by themselves on this lot (the update and the running
    // changes). At Mom's house they do (the home log); at the club they do not (its logs:
    // 1 of 47 motive values changes). What decides it is the update gate's third test, a
    // virtual call not identified yet (docs/simulation.md, "The motives"); until then the lot's
    // loader says.
    bool motives_run = true;
    std::vector<int16_t> globals;        // scope 6
    std::vector<int16_t> neighbourhood;  // scope 34
    std::vector<Neighbour> neighbours;   // the neighbourhood's, in id order (read_neighbours)
    std::vector<int16_t> ad_range;       // scope 12: one shared value per datum (only 15 is used)

private:
    const Library& lib_;
    std::vector<std::unique_ptr<Object>> objects_;
    std::map<int16_t, Object*> by_id_;
    uint64_t version_ = 0;
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
