// The behaviour interpreter's core: frames, calls, returns and the node step, as
// docs/interpreter.md describes the engine's (TreeSimImpl::DoNodeAction). The primitives
// are a host's: each object (and person) answers them, and finds the trees it may call.
#pragma once
#include <cstdint>
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
};

class Interpreter;

class Host {
public:
    virtual ~Host() = default;
    virtual PrimResult primitive(Interpreter& in, Frame& f, const Node& n) = 0;
    // The tree a call names (tree_kind says which file); nullptr if there is none.
    virtual const Tree* tree(uint16_t id) = 0;
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
    void start(const Tree* t, const std::vector<int16_t>& params = {});
    // Push a call (what a call node does, and what a primitive may do before Continue).
    // Parameters: the given ones, or none (empty; see push's note in interp.cpp).
    bool push(const Tree* t, const int16_t* params, int count);

    Step step();
    // Steps until the tree yields or stops; then the number of steps run.
    int run(int max_steps = 1 << 20);

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
};

}  // namespace housefire
