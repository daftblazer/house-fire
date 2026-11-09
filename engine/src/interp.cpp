#include "housefire/interp.h"

namespace housefire {

void Interpreter::start(const Tree* t, const std::vector<int16_t>& params) {
    stack_.clear();
    finished_ = false;
    error_ = Error::None;
    iterations_ = 0;
    push(t, params.data(), (int)params.size());
}

// The engine's call (Bustin' Out's fn_8023EE1C, TreeSimImpl::Gosub): the frame's header is
// made from the tree (its parameter and local counts), the parameters given are copied in,
// and the first node is 0, or for a tree of type 4 the first parameter.
bool Interpreter::push(const Tree* t, const int16_t* params, int count) {
    if (!t) return false;
    Frame f;
    f.tree = t;
    f.params.assign(t->args, 0);
    for (int i = 0; i < count && i < (int)f.params.size(); i++) f.params[i] = params[i];
    int node = 0;
    if (t->type == 4) {
        node = count > 0 ? params[0] : 0;
        if (node >= (int)t->nodes.size()) node = (int)t->nodes.size() - 1;
    }
    if (node < 0 || node >= (int)t->nodes.size()) return false;
    f.node = (uint16_t)node;
    // The engine does not clear a new frame's locals: they are what the stack's memory held
    // (measured: 11,267 of 11,554 calls whose locals were not zero). Zero here until the
    // stack is kept as the engine's memory is (docs/interpreter.md).
    f.locals.assign(t->locals, 0);
    stack_.push_back(std::move(f));
    return true;
}

Step Interpreter::fail(Error e) {
    error_ = e;
    finished_ = true;
    return Step::Stop;
}

// A node's true or false target becomes the current node; 253 is the error exit.
Step Interpreter::transition(Frame& f, bool value) {
    const Node& n = f.tree->nodes[f.node];
    const uint8_t t = value ? n.true_target : n.false_target;
    if (t == kTargetError) return fail(Error::BadTransition);
    f.node = t;
    return Step::Go;
}

Step Interpreter::step() {
    if (finished_ || stack_.empty()) return Step::Stop;
    Frame& f = stack_.back();

    // A return.
    if (f.node > 250) {
        if (f.node != kReturnTrue && f.node != kReturnFalse) return fail(Error::BadTransition);
        const bool value = f.node == kReturnTrue;
        last_result_ = value;
        if (stack_.size() <= 1) {
            if (mode_ == Mode::Main) return fail(Error::ReturnFromBottom);
            stack_.pop_back();
            finished_ = true;
            return Step::Stop;
        }
        stack_.pop_back();
        return transition(stack_.back(), value);
    }

    if (f.node >= f.tree->nodes.size()) return fail(Error::Missing);
    if (++iterations_ > max_iterations_) return fail(Error::TooManyIterations);
    const Node& n = f.tree->nodes[f.node];
    const uint16_t op = n.opcode & 0x7FFF;

    // A call: the operand's four fields are the parameters, unless both its words are -1.
    if (op > 255) {
        const Tree* t = host_.tree(op);
        if (!t) return fail(Error::Missing);
        bool none = true;
        for (uint8_t b : n.operand) none = none && b == 0xFF;
        int16_t p[4];
        for (int i = 0; i < 4; i++) p[i] = (int16_t)n.operand16(i);
        const std::span<const int16_t> d = host_.default_params();
        if (!push(t, none ? d.data() : p, none ? (int)d.size() : 4)) return fail(Error::CallFailed);
        return Step::Go;
    }

    // A primitive.
    switch (host_.primitive(*this, f, n)) {
        case PrimResult::True: return transition(stack_.back(), true);
        case PrimResult::False: return transition(stack_.back(), false);
        case PrimResult::Yield: return Step::Yield;
        case PrimResult::Continue: return Step::Go;
        default: finished_ = true; return Step::Stop;
    }
}

int Interpreter::run(int max_steps) {
    int i = 0;
    for (; i < max_steps; i++) {
        const Step s = step();
        if (s != Step::Go) return i + 1;
    }
    return i;
}

}  // namespace housefire
