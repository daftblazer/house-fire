// The readers and the interpreter on the player's own Bustin' Out disc (HOUSEFIRE_BUSTIN_ISO;
// skipped if it is not set). Nothing from the disc is kept.
//
// Every BHAV on the disc must read (9,529, as tools/check_chunks.py counts), and every tree
// is run as a check tree with a host whose primitives answer by a fixed rule (true for an
// even opcode, false for an odd one) and whose calls find the trees as the engine would
// (own: the object's file; semi-global: the file its GLOB names; global: Global.iff). Each
// run must end: by a return, by one of the node step's errors, or at the iteration limit.
#include <cstdio>
#include <cstdlib>
#include <map>
#include <memory>

#include "housefire/disc.h"
#include "housefire/interp.h"

using namespace housefire;

struct File {
    std::vector<uint8_t> bytes;
    IffFile iff;
    std::map<uint16_t, Tree> trees;
    std::string glob;  // the semi-global file's name
};

struct RuleHost : Host {
    const File* own = nullptr;
    const File* semi = nullptr;
    const File* global = nullptr;
    PrimResult primitive(Interpreter&, Frame&, const Node& n) override {
        return n.opcode % 2 ? PrimResult::False : PrimResult::True;
    }
    const Tree* in(const File* f, uint16_t id) {
        if (!f) return nullptr;
        auto it = f->trees.find(id);
        return it == f->trees.end() ? nullptr : &it->second;
    }
    const Tree* tree(uint16_t id) override {
        switch (tree_kind(id)) {
            case TreeKind::Global: return in(global, id);
            case TreeKind::Own: return in(own, id);
            case TreeKind::SemiGlobal: return in(semi, id);
            default: return nullptr;
        }
    }
};

static std::string lower(std::string s) {
    for (char& c : s) c = (char)std::tolower((unsigned char)c);
    return s;
}

int main() {
    const char* iso = std::getenv("HOUSEFIRE_BUSTIN_ISO");
    if (!iso || !*iso) { std::printf("test_disc: skipped (HOUSEFIRE_BUSTIN_ISO not set)\n"); return 0; }
    Disc d(iso);
    if (!d.ok() || d.id() != "G4ME69") { std::printf("test_disc: %s is not Bustin' Out\n", iso); return 1; }

    std::vector<std::unique_ptr<File>> all;
    std::map<std::string, File*> files;  // by lower-case name without .iff (the first of a name)
    int bhavs = 0, bad = 0;
    for (const auto& [path, where] : d.files()) {
        if (path.rfind("Runtime/", 0) != 0 || path.size() < 5 || path.substr(path.size() - 4) != ".iff") continue;
        auto f = std::make_unique<File>();
        f->bytes = d.read(path);
        f->iff = read_iff(f->bytes);
        const Order o = bhav_order(f->iff);
        for (const Chunk* c : f->iff.of("BHAV")) {
            try { f->trees[(uint16_t)c->id] = read_bhav(*c, o); bhavs++; }
            catch (const FormatError& e) { bad++; std::printf("  %s %d: %s\n", path.c_str(), c->id, e.what()); }
        }
        for (const Chunk* c : f->iff.of("GLOB"))
            if (!c->data.empty()) {
                const size_t n = c->data[0];
                if (n + 1 <= c->data.size()) f->glob.assign((const char*)c->data.data() + 1, n);
            }
        const size_t slash = path.rfind('/');
        files.emplace(lower(path.substr(slash + 1, path.size() - slash - 5)), f.get());
        all.push_back(std::move(f));
    }
    std::printf("BHAV read: %d, failed %d (tools/check_chunks.py: 9529)\n", bhavs, bad);

    const File* global = files.count("global") ? files["global"] : nullptr;
    std::map<std::string, int> outcome;
    long steps = 0;
    for (const auto& f : all) {
        RuleHost h;
        h.own = f.get();
        h.global = global;
        if (!f->glob.empty() && files.count(lower(f->glob))) h.semi = files[lower(f->glob)];
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
