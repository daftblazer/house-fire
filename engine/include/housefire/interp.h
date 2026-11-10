// The behaviour interpreter's core: frames, calls, returns and the node step, as
// docs/interpreter.md describes the engine's (TreeSimImpl::DoNodeAction). The primitives
// are a host's: each object (and person) answers them, and finds the trees it may call.
#pragma once
#include <cstdint>
#include <span>
#include <vector>

#include "housefire/bhav.h"

namespace housefire {

// What a primitive answers (the engine's values).
enum class PrimResult : int { False = 0, True = 1, Yield = 2, Continue = 3, Stop = 4 };

// The node step's error codes (stored by the engine at +52 of its interpreter).
enum class Error : uint16_t {
    None = 0,
    CallFailed = 1000,       // a call to a tree that exists could not be made
    ReturnFromBottom = 1001, // the bottom frame returned (the main tree must not)
    BadTransition = 1100,    // a target of 253, or a return value other than 254 or 255
    TooManyIterations = 1101,
    Missing = 1102,          // no such node, or no such tree
};

struct Frame {
    const Tree* tree = nullptr;
    uint16_t node = 0;
    std::vector<int16_t> params, locals;
    // Locals start at zero (the original leaves whatever its stack's memory held: see
    // docs/interpreter.md, "The call"). Bit k is set while local k has not been written in
    // this frame, so that a read of one is reported (Host::unset_local) and a script that
    // depends on the original's left-over values can be found. Locals past 63 are not tracked.
    uint64_t unset = 0;
    // The stack object (scope 10): an object id, the frame's own, a call's taken from its
    // caller (the original keeps it in the frame's header, +4).
    int16_t stack_object = 0;
    // The object file the frame's trees are found in and its constants read from (the
    // original's +12): a call's is its caller's.
    const struct ObjectFile* file = nullptr;
    // A frame the engine started rather than a script (the original's +8).
    bool engine = false;
};

class Interpreter;

class Host {
public:
    virtual ~Host() = default;
    virtual PrimResult primitive(Interpreter& in, Frame& f, const Node& n) = 0;
    // The tree a call names (tree_kind says which file); nullptr if there is none.
    virtual const Tree* tree(uint16_t id) = 0;
    // The parameters a call that gives none passes: the interpreter's own, given when it is
    // set up (TreeSimImpl::Initialize keeps a pointer at +44; Bustin' Out's call, fn_8023EE1C,
    // copies from it). In the PC game they are the object's temporaries.
    virtual std::span<const int16_t> default_params() { return {}; }
    // A primitive read local k of `f` before anything wrote it (tools/locals_check.py finds
    // the scripts that can do this; on the disc they read zero in the original too).
    virtual void unset_local(const Frame& f, int k) { (void)f; (void)k; }
};

enum class Step { Go, Yield, Stop };

class Interpreter {
public:
    // A main tree's bottom frame may not return (error 1001); a check tree's return is its
    // result (`Check`).
    enum class Mode { Main, Check };

    Interpreter(Host& host, Mode mode = Mode::Main, uint32_t max_iterations = 10000)
        : host_(host), mode_(mode), max_iterations_(max_iterations) {}

    // Start `t` as the bottom frame with these parameters.
    void start(const Tree* t, const std::vector<int16_t>& params = {}, const struct ObjectFile* file = nullptr);
    // Push a call (what a call node does, and what a primitive may do before Continue), with
    // these parameters (the tree's count of them is taken, the rest zero). A tree of type 4
    // starts at the node its first parameter names (the last if past the end); false if
    // that is negative or the tree has no nodes.
    bool push(const Tree* t, const int16_t* params, int count);
    // Take over a stack as it stands (a saved game's, or the original's in a lockstep check).
    void restore(std::vector<Frame> frames) {
        stack_ = std::move(frames);
        finished_ = false;
        error_ = Error::None;
    }

    Step step();
    // Steps until the tree yields or stops; then the number of steps run.
    int run(int max_steps = 1 << 20);

    // A frame's locals, for the primitives: reads report an unset local to the host.
    int16_t local(Frame& f, int k) {
        if (k < 64 && (f.unset >> k & 1)) host_.unset_local(f, k);
        return k < (int)f.locals.size() ? f.locals[k] : 0;
    }
    void set_local(Frame& f, int k, int16_t v) {
        if (k >= (int)f.locals.size()) return;
        f.locals[k] = v;
        if (k < 64) f.unset &= ~(uint64_t(1) << k);
    }

    // A stack that sleeps (Idle): not stepped until the clock reaches `wake`. The clock is
    // the simulation's ticks, which stop while the game is paused.
    void sleep_until(uint32_t wake) { wake_ = wake; sleeping_ = true; }
    bool asleep(uint32_t now) const { return sleeping_ && now < wake_; }
    bool woke() const { return sleeping_; }  // slept, and is being stepped again
    void wake() { sleeping_ = false; }

    bool done() const { return finished_; }
    bool result() const { return last_result_; }  // the last return (or a check tree's answer)
    Error error() const { return error_; }
    uint32_t iterations() const { return iterations_; }
    const std::vector<Frame>& stack() const { return stack_; }
    Frame& top() { return stack_.back(); }
    void reset_iterations() { iterations_ = 0; }

private:
    Step fail(Error e);
    Step transition(Frame& f, bool value);

    Host& host_;
    Mode mode_;
    uint32_t max_iterations_;
    uint32_t iterations_ = 0;
    std::vector<Frame> stack_;
    bool last_result_ = false;
    bool finished_ = false;
    Error error_ = Error::None;
    uint32_t wake_ = 0;
    bool sleeping_ = false;
};

}  // namespace housefire
