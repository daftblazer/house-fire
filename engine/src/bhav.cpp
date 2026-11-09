#include "housefire/bhav.h"

namespace housefire {

uint16_t Node::operand16(int i) const {
    const uint8_t* b = operand.data() + 2 * i;
    return order == Order::Little ? (uint16_t)(b[0] | b[1] << 8) : (uint16_t)(b[0] << 8 | b[1]);
}

TreeKind tree_kind(uint16_t id) {
    if (id <= 255) return TreeKind::Primitive;
    if (id <= 4095) return TreeKind::Global;
    if (id <= 8191) return TreeKind::Own;
    if (id <= 9000) return TreeKind::SemiGlobal;
    return TreeKind::None;
}

Tree read_bhav(const Chunk& c, Order order) {
    Reader r(c.data, order);
    Tree t;
    t.id = c.id;
    t.signature = r.u16();
    if (t.signature < 0x8000 || t.signature > 0x8002) throw FormatError("BHAV signature");
    const uint16_t n = r.u16();
    if ((size_t)n * 12 > r.left()) throw FormatError("BHAV node count");
    t.type = r.u8();
    t.args = r.u8();
    t.locals = r.u16();
    t.version = r.u32();
    t.nodes.reserve(n);
    for (uint16_t i = 0; i < n; i++) {
        Node nd;
        nd.opcode = r.u16();
        nd.true_target = r.u8();
        nd.false_target = r.u8();
        auto op = r.take(8);
        for (int k = 0; k < 8; k++) nd.operand[k] = op[k];
        nd.order = order;
        t.nodes.push_back(nd);
    }
    r.done();
    return t;
}

Order bhav_order(const IffFile& f) {
    for (const Chunk* c : f.of("BHAV"))
        if (c->data.size() >= 2) return c->data[1] == 0x80 ? Order::Little : Order::Big;
    return Order::Little;
}

}  // namespace housefire
