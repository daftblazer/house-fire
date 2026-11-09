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
    const bool ok = bad == 0 && bhavs == 9529 && total == bhavs && ended == total;
    std::printf("test_disc: %s\n", ok ? "ok" : "FAILED");
    return ok ? 0 : 1;
}
