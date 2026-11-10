// The object files a game's scripts run from, and the lookups the scripts make in them:
// trees by id (docs/interpreter.md, "Tree ids") and constants (scope 26).
#pragma once
#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "housefire/bhav.h"
#include "housefire/disc.h"
#include "housefire/iff.h"

namespace housefire {

// An object definition (OBJD): its version and 16-bit fields as the file has them. Fields
// 12-13 are the GUID (low half first).
struct Definition {
    int16_t id = 0;
    uint32_t version = 0;
    std::vector<uint16_t> fields;
    uint32_t guid() const { return fields.size() > 13 ? (uint32_t)fields[12] | (uint32_t)fields[13] << 16 : 0; }
    uint16_t field(int i) const { return i >= 0 && (size_t)i < fields.size() ? fields[(size_t)i] : 0; }
    // The interaction table (TTAB chunk id) the object uses, of its file's: field 5. Measured
    // on the home log's three autonomous choices (docs/simulation.md, "Autonomy").
    uint16_t interaction_table() const { return field(5); }
};

// An interaction of a table (TTAB): the action and guard trees, its flags, how its appeal
// falls off with distance (attenuation code and value), the motive level below which a Sim
// considers it by itself (autonomy threshold), and what it advertises: for each motive, a
// minimum, a delta and the personality field that scales it.
struct Interaction {
    uint16_t action = 0, guard = 0;
    uint32_t flags = 0;
    int32_t string_index = 0, attenuation_code = 0;
    float attenuation_value = 0;
    int32_t autonomy_threshold = 0, joining_index = 0;
    struct Advert { int16_t min = 0, delta = 0; uint16_t personality = 0; };
    std::vector<Advert> adverts;  // by motive
};

struct ObjectFile {
    std::string path;                                    // on the disc
    std::vector<uint8_t> bytes;
    IffFile iff;
    std::map<uint16_t, Tree> trees;                      // BHAV by id
    std::map<uint16_t, std::vector<int16_t>> constants;  // BCON by id
    std::map<uint16_t, std::vector<std::string>> strings;  // STR# by id (every language's, in turn)
    // Named tuning values (FCNS) by chunk id: the global file's motive rates (1), autonomy (2),
    // rooms and light (3), the family's score (4); docs/simulation.md, "The motives".
    std::map<uint16_t, std::vector<std::pair<std::string, float>>> tuning;
    std::map<std::string, uint16_t> tree_by_name;        // BHAV ids by their chunk labels
    // From the interaction tables (TTAB): an interaction's action tree -> its guard (test) tree,
    // and -> the tree its table names to run first (version 11's field for the whole table).
    std::map<uint16_t, uint16_t> guard_of, prefix_of;
    // The file's interactions in its table's order (action, guard), if it has one TTAB.
    std::vector<std::pair<uint16_t, uint16_t>> interactions;
    int ttabs = 0;
    std::map<uint16_t, std::vector<Interaction>> tables;  // TTAB by chunk id
    std::vector<Definition> definitions;                 // OBJD, in the file's order
    std::string glob;                                    // the semi-global file it names
    const ObjectFile* semi = nullptr;                    // that file, if there is one
};

// A GLOB's name: length-prefixed ("\rPersonGlobals") or zero-ended ("person_sg").
std::string glob_name(std::span<const uint8_t> data);

class Library {
public:
    // Bustin' Out's: every IFF file under Runtime/. Their semi-global files are linked by
    // name, except that the characters' PersonGlobals is person_sg, which the game uses
    // (docs/interpreter.md, "Semi-global files").
    static Library bustin_out(const Disc& d);

    const std::vector<std::unique_ptr<ObjectFile>>& files() const { return files_; }
    const ObjectFile* global() const { return global_; }
    const ObjectFile* find(const std::string& name) const;  // by file name, any case, no .iff
    int bhav_failures() const { return bhav_failures_; }
    int tuning_failures() const { return tuning_failures_; }  // FCNS chunks not read to their end
    // A named tuning value of the global file's (FCNS), if it has one.
    std::optional<float> tuning(std::string_view name) const;
    // The interaction table a definition uses: its field 5 names it; a multi-tile object's part
    // (0xFFFF there) uses its master definition's (the file's definition with the same master
    // id, field 8, and a table), and a file with one table that one. Null if none.
    const std::vector<Interaction>* interactions_of(const Definition& d, const ObjectFile& file) const;
    // A definition by GUID, and the file it is in (the first if several files have it).
    const Definition* definition(uint32_t guid, const ObjectFile** file = nullptr) const;

    // A tree as a script of `object` names it: global, the object's own, or its
    // semi-global file's, by the id's range.
    const Tree* tree(const ObjectFile* object, uint16_t id) const;
    // A constant as `object`'s scripts name it: the datum's top 9 bits a table (0-63 the
    // object's own BCON 4096 on, 64-127 its semi-global file's 8192 on, 128 on the global
    // file's 256 on), its low 7 bits the entry. Checked against the recompiled game.
    std::optional<int16_t> constant(const ObjectFile* object, int16_t data) const;

private:
    std::vector<std::unique_ptr<ObjectFile>> files_;
    std::map<std::string, ObjectFile*> by_name_;
    std::map<uint32_t, std::pair<const ObjectFile*, size_t>> by_guid_;
    const ObjectFile* global_ = nullptr;
    int bhav_failures_ = 0;
    int tuning_failures_ = 0;
};

}  // namespace housefire
