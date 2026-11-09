// The object files a game's scripts run from, and the lookups the scripts make in them:
// trees by id (docs/interpreter.md, "Tree ids") and constants (scope 26).
#pragma once
#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <span>
#include <string>
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
};

struct ObjectFile {
    std::string path;                                    // on the disc
    std::vector<uint8_t> bytes;
    IffFile iff;
    std::map<uint16_t, Tree> trees;                      // BHAV by id
    std::map<uint16_t, std::vector<int16_t>> constants;  // BCON by id
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
};

}  // namespace housefire
