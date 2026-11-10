// The lots of the player's Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO; skipped if not set): the
// neighbourhood file's blocks, every lot's object types and objects, and a world made from
// each lot. The counts are tools/ngh.py's, lot.py's and objm.py's; the club's object order
// is the running game's (docs/loop.md, iteration 5).
#include <cstdio>
#include <cstdlib>

#include "housefire/disc.h"
#include "housefire/world.h"

using namespace housefire;

int main() {
    const char* iso = std::getenv("HOUSEFIRE_BUSTIN_ISO");
    if (!iso || !*iso) { std::printf("test_lots: skipped (HOUSEFIRE_BUSTIN_ISO not set)\n"); return 0; }
    Disc d(iso);
    if (!d.ok() || d.id() != "G4ME69") { std::printf("test_lots: %s is not Bustin' Out\n", iso); return 1; }
    const Library lib = Library::bustin_out(d);
    const std::vector<uint8_t> ngh = d.read("Runtime/default.ngh");
    bool ok = true;
    try {
        const auto blocks = read_neighbourhood(ngh);
        const auto lots = read_lots(ngh);
        size_t types = 0, objects = 0, missing = 0;
        // The saved tree stacks: frames, frames whose tree is found (the object's own file,
        // its semi-global or the global one), and found frames whose node and counts fit it.
        size_t frames = 0, found = 0, fit = 0, person_frames = 0;
        std::vector<int16_t> club;
        for (const LotChunks& lot : lots) {
            types += read_object_types(*lot.find("objt")).size();
            World w(lib);
            missing += (size_t)w.load_lot(lot);
            objects += w.objects().size();
            for (const auto& o : w.objects())
                for (const Frame& f : o->stack) {
                    frames++;
                    if (o->person) { person_frames++; continue; }
                    if (!f.tree) continue;
                    found++;
                    fit += f.node < f.tree->nodes.size() && f.params.size() == f.tree->args &&
                           f.locals.size() == f.tree->locals;
                }
            if (&lot == &lots.back())
                for (size_t k = 0; k < 6 && k < w.objects().size(); k++) club.push_back(w.objects()[k]->id());
        }
        std::printf("blocks %zu, lots %zu, object types %zu, objects %zu (%zu without a definition on the disc)\n",
                    blocks.size(), lots.size(), types, objects, missing);
        std::printf("tree stack frames %zu (%zu of characters), found %zu, fitting their tree %zu\n",
                    frames, person_frames, found, fit);
        std::printf("the club's first objects:");
        for (int16_t id : club) std::printf(" %d", id);
        std::printf("\n");
        ok = blocks.size() == 35 && lots.size() == 16 && types == 2000 && objects == 4572 &&
             club == std::vector<int16_t>{178, 177, 176, 174, 172, 171} &&
             frames == 4575 && person_frames == 53 && found == 4522 && fit == 4522;
    } catch (const FormatError& e) {
        std::printf("FormatError: %s\n", e.what());
        ok = false;
    }
    std::printf("test_lots: %s\n", ok ? "ok" : "FAILED");
    return ok ? 0 : 1;
}
