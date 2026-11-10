// The readers and the interpreter on the player's own Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO;
// skipped if it is not set). Nothing from the disc is kept.
//
// Every BHAV on the disc must read (9,529, as tools/check_chunks.py counts), and every tree
// is run as a check tree with a host whose primitives answer by a fixed rule (true for an
// even opcode, false for an odd one) and whose calls find the trees as the engine would
// (the library's lookup: own, semi-global, global by the id's range). Each
// run must end: by a return, by one of the node step's errors, or at the iteration limit.
#include <cstdio>
#include <cstdlib>
#include <map>
#include <memory>

#include "housefire/disc.h"
#include "housefire/interp.h"
#include "housefire/library.h"

using namespace housefire;

struct RuleHost : Host {
    const Library* lib = nullptr;
    const ObjectFile* own = nullptr;
    PrimResult primitive(Interpreter&, Frame&, const Node& n) override {
        return n.opcode % 2 ? PrimResult::False : PrimResult::True;
    }
    const Tree* tree(uint16_t id) override { return lib->tree(own, id); }
};

int main() {
    const char* iso = std::getenv("HOUSEFIRE_BUSTIN_ISO");
    if (!iso || !*iso) { std::printf("test_disc: skipped (HOUSEFIRE_BUSTIN_ISO not set)\n"); return 0; }
    Disc d(iso);
    if (!d.ok() || d.id() != "G4ME69") { std::printf("test_disc: %s is not Bustin' Out\n", iso); return 1; }

    const Library lib = Library::bustin_out(d);
    int bhavs = 0;
    for (const auto& f : lib.files()) bhavs += (int)f->trees.size();
    const int bad = lib.bhav_failures();
    std::printf("BHAV read: %d, failed %d (tools/check_chunks.py: 9529)\n", bhavs, bad);

    int constants = 0;
    for (const auto& f : lib.files()) constants += (int)f->constants.size();
    std::printf("BCON read: %d\n", constants);

    std::map<std::string, int> outcome;
    long steps = 0;
    for (const auto& f : lib.files()) {
        RuleHost h;
        h.lib = &lib;
        h.own = f.get();
        for (const auto& [id, t] : f->trees) {
            Interpreter in(h, Interpreter::Mode::Check, 2000);
            in.start(&t);
            steps += in.run(100000);
            if (!in.done()) outcome["did not end"]++;
            else if (in.error() == Error::None) outcome[in.result() ? "returned true" : "returned false"]++;
            else outcome["error " + std::to_string((int)in.error())]++;
        }
    }
    int ended = 0, total = 0;
    for (const auto& [k, v] : outcome) {
        std::printf("  %-16s %d\n", k.c_str(), v);
        total += v;
        if (k != "did not end") ended += v;
    }
    std::printf("trees run: %d, %ld steps; all ended: %s\n", total, steps, ended == total ? "yes" : "no");
    // String tables (STR#): the two CallNamedTree reads in the logged session, 303 in the party
    // controller (5 strings) and in PersonGlobals (1), as tools/chunks.py reads them.
    auto table = [&](const char* file, uint16_t id) -> long {
        const ObjectFile* f = lib.find(file);
        if (!f) return -1;
        auto t = f->strings.find(id);
        return t == f->strings.end() ? -1 : (long)t->second.size();
    };
    std::printf("STR# 303: controller_party %ld, PersonGlobals %ld\n", table("controller_party", 303), table("PersonGlobals", 303));
    // Tuning tables (FCNS): every one read to its end; the global file's four (17, 12, 19 and 9
    // values), and the motive rates the home log measured (docs/simulation.md, "The motives").
    size_t tuning_tables = 0, tuning_values = 0;
    for (const auto& f : lib.files())
        for (const auto& [id, t] : f->tuning) { tuning_tables++; tuning_values += t.size(); }
    auto tv = [&](const char* n) { return lib.tuning(n).value_or(-1.0f); };
    std::printf("FCNS: %zu tables, %zu values, %d not read; hyg decrement, awake %g; hunger decrement ratio %g; energy span %g\n",
                tuning_tables, tuning_values, lib.tuning_failures(), tv("hyg decrement, awake"), tv("hunger decrement ratio"), tv("energy span"));
    // Interaction tables (TTAB): the home log's three autonomous choices in their objects'
    // tables (OBJD field 5), each advertising what the Sim lacked (docs/simulation.md,
    // "Autonomy"): the fridge's snack hunger +20, the toilet bladder +70, the easel fun +10.
    auto advert = [&](uint32_t guid, uint16_t action, int motive) -> int {
        const ObjectFile* file = nullptr;
        const Definition* def = lib.definition(guid, &file);
        if (!def || !file) return -999;
        auto t = file->tables.find(def->interaction_table());
        if (t == file->tables.end()) return -998;
        for (const Interaction& e : t->second)
            if (e.action == action) return (size_t)motive < e.adverts.size() ? e.adverts[(size_t)motive].delta : -997;
        return -996;
    };
    const int fridge = advert(0x675C18AF, 8215, 7), toilet = advert(0x3C566968, 8223, 9), easel = advert(0x9FB223CE, 4097, 15);
    size_t tables = 0;
    for (const auto& f : lib.files()) tables += f->tables.size();
    std::printf("TTAB: %zu tables; the fridge's snack hunger %d, the toilet bladder %d, the easel fun %d\n", tables, fridge, toilet, easel);
    // Slots (SLOT): every one of version 10 and read to its end; the global file's 15.
    size_t slot_tables = 0;
    for (const auto& f : lib.files()) slot_tables += f->slots.size();
    const size_t global_slots = lib.global() && lib.global()->slots.size() == 1 ? lib.global()->slots.begin()->second.size() : 0;
    std::printf("SLOT: %zu tables, %d not read; the global file's %zu slots\n", slot_tables, lib.slot_failures(), global_slots);
    const bool ok = bad == 0 && bhavs == 9529 && total == bhavs && ended == total &&
                    table("controller_party", 303) == 5 && table("PersonGlobals", 303) == 1 &&
                    lib.tuning_failures() == 0 && lib.global() && lib.global()->tuning.size() == 4 &&
                    tv("hyg decrement, awake") == 0.17f && tv("hunger decrement ratio") == 0.0021f && tv("energy span") == 180.0f &&
                    fridge == 20 && toilet == 70 && easel == 10 && lib.slot_failures() == 0 && global_slots == 15;
    std::printf("test_disc: %s\n", ok ? "ok" : "FAILED");
    return ok ? 0 : 1;
}
