// The engine's saved data: the neighbourhood file (Bustin' Out's Runtime/default.ngh), its
// lots, and the save stream they are written in (docs/lots.md; tools/ngh.py, recon.py,
// lot.py and objm.py are the same, in Python).
#pragma once
#include <cstdint>
#include <functional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace housefire {

// The save stream (The Sims 2's ReconBuffer): bits from the top of each byte; with
// compression on, a value is 0 (bit 0) or bit 1, a two-bit width code and that many bits,
// sign-extended (8-bit 2/4/6/8, 16-bit 5/8/13/16, 32-bit 6/11/21/32); a mark pads to a
// byte and is a plain 32-bit word. Reads past the end throw FormatError.
class SaveStream {
public:
    SaveStream(std::span<const uint8_t> d, size_t pos = 0) : d_(d), pos_(pos) {}
    bool compressed = false;
    int32_t r8();
    int32_t r16();
    int32_t r32();
    bool rbool() { return r8() != 0; }
    float rfloat();
    uint32_t mark();  // a plain little-endian word
    void pad() { mask_ = 0; }  // on to the next byte
    size_t pos() const { return pos_; }

private:
    int bit();
    uint32_t bits(int n);
    int32_t packed(int size);
    uint32_t raw(int n);
    std::span<const uint8_t> d_;
    size_t pos_;
    uint8_t cur_ = 0, mask_ = 0;
};

// LZSS as the neighbourhood file packs its blocks: flag bytes (low bit first, 1 a literal),
// references of a 12-bit position and a length of 3-18, a 4096-byte ring from 0xFEE.
std::vector<uint8_t> unpack_lzss(std::span<const uint8_t> packed, size_t size, size_t* used = nullptr);

// One lot of the neighbourhood: its chunks by FourCC (Arry x3, objt, ObjM).
struct LotChunks {
    uint32_t block = 0;  // its block's offset in the file
    std::vector<std::pair<std::string, std::vector<uint8_t>>> chunks;
    const std::vector<uint8_t>* find(const std::string& type) const;
};

// The neighbourhood file's blocks: section, count, unpacked data. Throws FormatError on a
// block that does not unpack to exactly its stated size.
struct NeighbourhoodBlock {
    uint32_t offset, section, count;
    bool big_endian = false;  // a save's (memory_card_file), not the disc's
    std::vector<uint8_t> data;
};
std::vector<NeighbourhoodBlock> read_neighbourhood(std::span<const uint8_t> file);
// Its lots (section 3), each by its chunk table.
std::vector<LotChunks> read_lots(std::span<const uint8_t> file);

// A save on a GameCube memory card image (a raw 2 MB card): the file of `game` (its six-byte
// game and maker code, "G4ME69") by the card's directory and block map; empty if the card has
// none. Bustin' Out's save is the neighbourhood file as the game keeps it in memory: the same
// blocks and chunks as the disc's, but its block headers and chunk tables big-endian (and
// the FourCCs not reversed); the chunks' own contents are the disc's formats.
std::vector<uint8_t> memory_card_file(std::span<const uint8_t> card, std::string_view game);

// A neighbour of the neighbourhood (its NBRS chunk, the PC's layout: docs/objects.md, "The
// neighbours"): its id, name (the character file's), GUID, person data (if saved) and its
// relationships with other neighbours.
struct Neighbour {
    int16_t id = 0;
    std::string name;
    uint32_t guid = 0;
    std::vector<int16_t> person_data;
    struct Relationship {
        int16_t with = 0;
        std::vector<int32_t> values;
    };
    std::vector<Relationship> relationships;
};
// The neighbours, in id order. Throws FormatError if NBRS does not read to its end.
std::vector<Neighbour> read_neighbours(std::span<const uint8_t> file);

// One of a lot's three arrays (its Arry chunks): a width by height grid (64 by 64 on the
// disc) of elements of a size (1, 8, 1), as bytes, row after row. Read from their run-length
// coding (docs/lots.md, "The arrays"); what each holds is to be named (the floors, the walls,
// the ground, by their values). Throws FormatError if one does not read to its chunk's end.
struct LotArray {
    uint32_t width = 0, height = 0, element_size = 0;
    std::vector<uint8_t> bytes;
};
std::vector<LotArray> read_lot_arrays(const LotChunks& lot);

// An object type of a lot (objt): its GUID, six 16-bit fields (field 4 is the type number
// the lot's objects name) and its name.
struct ObjectType {
    uint32_t guid = 0;
    uint16_t fields[6] = {};
    std::string name;
};
std::vector<ObjectType> read_object_types(std::span<const uint8_t> objt);

// A frame of an object's saved tree stack (docs/lots.md, "An object's section"). The tree is
// named by its id as the object's scripts name it; the file the original saves with it (an
// index into its list of loaded files, which depends on load order) is not kept: a frame's
// file is its object's.
struct SavedFrame {
    int16_t stack_object = 0;
    uint16_t tree = 0, node = 0;
    std::vector<int16_t> params, locals;
    bool engine = false;  // a frame the engine started (the original's +8)
};

// What the loader makes an object of, which decides what its section holds after the base
// object's part (World::saved_class gives it from the definition).
enum class SavedClass : uint8_t {
    Object,  // the base object only
    Part,    // a part of a multi-tile object (its definition has a master id)
    Door,    // a part of a multi-tile door
    Sim,     // a character (no definition on the disc)
};

// An entry of a Sim's interaction queue as saved (fn_801CFB68; Castaway's Interaction): an
// id, three objects (the Sim, the interaction's target, which is the pushed frame's stack
// object, and a third), the tree the interaction runs (in the target's file), the rest to be
// named (c is 100 in every saved entry: a priority?). Checked: each queued Sim's first
// interaction is its first entry's tree on its target (docs/simulation.md).
struct SavedQueueEntry {
    int32_t a = 0, id = 0, b = 0, c = 0, flags = 0;
    int16_t objects[3] = {}, shorts[4] = {}, tree = 0;
    float e = 0;
    // Not saved: the interaction's place in the target's table, when the engine queued it
    // (PushAction); -1 for a saved entry. An action can be in a table more than once, each
    // with its own guard (the group meal's 4201: guards 4202 and 8248).
    int interaction = -1;
};

// An object's saved relationship with another (the save's RelArray): the other object's id
// and the relationship's values.
struct SavedRelationship {
    int32_t with = 0;
    std::vector<int32_t> values;
};

// A lot's object as its table gives it, in the world's list order.
struct LotObject {
    int16_t id = 0;
    int32_t a = 0, b = 0;            // two 32-bit values (to be named)
    std::vector<int32_t> extra;      // counted 32-bit values
    int16_t type = 0;                // objt field 4
    size_t section = 0, section_end = 0;  // its section of the stream
    // The start of its section (fn_801F1E30's header, then fn_801F1F20's tables): its
    // attributes (as many as the save gives), its 8 temps and its 72 data fields.
    int32_t header[8] = {};
    std::vector<int16_t> attributes, temps, data;
    // Then its tree stack, bottom first, its relationships, and the rest of the base
    // object's part (to be named): a list of pairs, a 32-bit value, a list of 16-bit values.
    std::vector<SavedFrame> stack;
    std::vector<SavedRelationship> relationships;
    std::vector<std::pair<int16_t, int16_t>> pairs;
    std::vector<int16_t> shorts;
    // Where the base object's part ends; the class's own part runs from here to the
    // loader's int at the end of the section.
    size_t base_end = 0;
    SavedClass saved_class = SavedClass::Object;
    // A multi-tile object's part: its leader's id (0 for the leader itself).
    int16_t leader = 0;
    // A Sim's own part: its person data (scope 18), its motives (scope 14), a second table
    // of 16 floats (to be named), the entry it is on and its queue.
    std::vector<int16_t> person_data;
    std::vector<float> motives, motives_b;
    SavedQueueEntry current;
    std::vector<SavedQueueEntry> queue;
    // Its +1120 list (fn_80229D5C): the motives changing at a rate, each a motive, its change
    // a tick and the value it goes to (SetMotiveDelta's).
    struct MotiveDelta { int32_t motive; float rate, target; };
    std::vector<MotiveDelta> motive_deltas;
    // Its +1180 list (fn_8022989C): an object, a 32-bit value and a flag each (to be named).
    struct Entry1180 { int16_t object; int32_t value; bool flag; };
    std::vector<Entry1180> list_1180;
    // Its +1160 list (fn_8022933C): only a count is saved; its elements are made at the load.
    int32_t list_1160 = 0;
};
constexpr int kObjectDataFields = 72, kObjectTemps = 8;
struct LotObjects {
    std::vector<LotObject> objects;
    size_t lot_section = 0, lot_section_end = 0;  // the lot's own part, after the objects
    // The lot's own part (the loader, fn_801FDE80, after the objects): an object, a list of
    // objects (both to be named; empty on the disc) and a string (UTF-16; empty on the disc).
    int16_t lot_object = 0;
    std::vector<int16_t> lot_objects;
    std::u16string lot_string;
};
// ObjM: the table, then the mark chain (one section per object, then the lot's). class_of
// gives the class of an objt type number. Every section must end exactly at its mark.
LotObjects read_lot_objects(std::span<const uint8_t> objm, const std::function<SavedClass(int16_t type)>& class_of);

}  // namespace housefire
