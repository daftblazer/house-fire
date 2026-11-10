//! Behaviour trees (BHAV chunks): `docs/interpreter.md`, `tools/chunks.py`.

use crate::iff::{FormatError, IffFile, Order, Reader};

/// Node targets above 250 end the tree.
pub const TARGET_ERROR: u8 = 253;
pub const RETURN_TRUE: u8 = 254;
pub const RETURN_FALSE: u8 = 255;

#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// Below 256 a primitive, above a tree ([`tree_kind`]).
    pub opcode: u16,
    pub true_target: u8,
    pub false_target: u8,
    /// Raw: its fields' order depends on the primitive.
    pub operand: [u8; 8],
    /// The operand's byte order (the file's).
    pub order: Order,
}

impl Node {
    /// The operand as four 16-bit fields.
    pub fn operand16(&self, i: usize) -> u16 {
        let b = [self.operand[2 * i], self.operand[2 * i + 1]];
        match self.order {
            Order::Little => u16::from_le_bytes(b),
            Order::Big => u16::from_be_bytes(b),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    Primitive,
    Global,
    Own,
    SemiGlobal,
    None,
}

/// 0-255 a primitive, 256-4095 global, 4096-8191 the object's own, 8192-9000 semi-global (The
/// Sims 2's Behavior::GetTreeClass and GetResFileID).
pub fn tree_kind(id: u16) -> TreeKind {
    match id {
        0..=255 => TreeKind::Primitive,
        256..=4095 => TreeKind::Global,
        4096..=8191 => TreeKind::Own,
        8192..=9000 => TreeKind::SemiGlobal,
        _ => TreeKind::None,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub id: i16,
    /// 0x8000-0x8002.
    pub signature: u16,
    pub kind: u8,
    pub args: u8,
    pub locals: u16,
    pub version: u32,
    pub nodes: Vec<Node>,
}

/// A BHAV chunk's tree; an error if it is not of a known form.
pub fn read_bhav(id: i16, data: &[u8], order: Order) -> Result<Tree, FormatError> {
    let mut r = Reader::new(data, order);
    let signature = r.u16()?;
    if !(0x8000..=0x8002).contains(&signature) {
        return Err(FormatError("BHAV signature".into()));
    }
    let n = r.u16()? as usize;
    if n * 12 > r.left() {
        return Err(FormatError("BHAV node count".into()));
    }
    let kind = r.u8()?;
    let args = r.u8()?;
    let locals = r.u16()?;
    let version = r.u32()?;
    let mut nodes = Vec::with_capacity(n);
    for _ in 0..n {
        let opcode = r.u16()?;
        let true_target = r.u8()?;
        let false_target = r.u8()?;
        let operand = r.take(8)?.try_into().unwrap();
        nodes.push(Node { opcode, true_target, false_target, operand, order });
    }
    r.done()?;
    Ok(Tree { id, signature, kind, args, locals, version, nodes })
}

/// A file's byte order, from its first BHAV's signature (0x80 second in little-endian files).
pub fn bhav_order(f: &IffFile) -> Order {
    f.of(b"BHAV")
        .map(|c| f.data(c))
        .find(|d| d.len() >= 2)
        .map_or(Order::Little, |d| if d[1] == 0x80 { Order::Little } else { Order::Big })
}
