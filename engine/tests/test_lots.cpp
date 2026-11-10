// The lots of the player's Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO; skipped if not set): the
// neighbourhood file's blocks, every lot's object types and objects, and a world made from
// each lot. The counts are tools/ngh.py's, lot.py's and objm.py's; the club's object order
// is the running game's (docs/loop.md, iteration 5).
#include <map>
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
        // The classes (every section read to exactly its mark), and the Sims' motives.
        size_t by_class[4] = {}, sims_with_motives = 0, lot_parts = 0, club_relationships = 0;
        std::vector<int16_t> club;
        for (const LotChunks& lot : lots) {
            types += read_object_types(*lot.find("objt")).size();
            World w(lib);
            missing += (size_t)w.load_lot(lot);
            objects += w.objects().size();
            {
                const auto lo = w.read_objects(lot);
                for (const LotObject& o : lo.objects) by_class[(int)o.saved_class]++;
                if (&lot == &lots.back())
                    for (const LotObject& o : lo.objects) club_relationships += o.relationships.size();
                // The lot's own part, read to the stream's end: on the disc no objects, and
                // an empty string.
                lot_parts += lo.lot_object == 0 && lo.lot_objects.empty() && lo.lot_string.empty();
            }
            for (const auto& o : w.objects())
                sims_with_motives += o->person && o->motives.size() == 16 && o->person_data.size() == 80;
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
        std::printf("classes: %zu objects, %zu parts, %zu door parts, %zu Sims (%zu with motives and person data)\n",
                    by_class[0], by_class[1], by_class[2], by_class[3], sims_with_motives);
        std::printf("lots' own parts as expected: %zu; the club's relationships: %zu\n", lot_parts, club_relationships);
        const auto nb = read_neighbours(ngh);
        std::printf("neighbours %zu, ids %d to %d\n", nb.size(), nb.empty() ? 0 : nb.front().id, nb.empty() ? 0 : nb.back().id);
        std::printf("the club's first objects:");
        for (int16_t id : club) std::printf(" %d", id);
        std::printf("\n");
        ok = blocks.size() == 35 && lots.size() == 16 && types == 2000 && objects == 4572 &&
             club == std::vector<int16_t>{178, 177, 176, 174, 172, 171} &&
             frames == 4575 && person_frames == 53 && found == 4522 && fit == 4522 &&
             by_class[0] == 1754 && by_class[1] == 1821 && by_class[2] == 944 && by_class[3] == 53 &&
             sims_with_motives == 53 && lot_parts == 16 && club_relationships == 16;
        ok = ok && nb.size() == 51 && nb.front().id == 3 && nb.back().id == 63;
    } catch (const FormatError& e) {
        std::printf("FormatError: %s\n", e.what());
        ok = false;
    }
    std::printf("test_lots: %s\n", ok ? "ok" : "FAILED");
    return ok ? 0 : 1;
}
