use std::collections::HashMap;

use crate::library::Library;
use crate::tree_log::{Log, StepRec, same_shape};
use crate::{FileId, TreeId};
use super::study::{env, atoi};

/// A logged tree on the disc: the file whose tree of that id has those nodes, by the nodes'
/// address and the tree's id. The game reuses an unloaded tree's memory for others, and the log
/// records a tree only the first time it is at an address, so a step's record can be another
/// tree's: only one of the step's own id is used.
pub(super) struct TreeFinder<'l> {
    pub(super) lib: &'l Library,
    pub(super) cache: HashMap<(u32, u16), Option<(TreeId, FileId)>>,
    pub(super) shown: i32,
}

impl TreeFinder<'_> {
    pub(super) fn find(&mut self, log: &Log, s: &StepRec) -> Option<(TreeId, FileId)> {
        if let Some(&r) = self.cache.get(&(s.nodes, s.tree)) {
            return r;
        }
        let t = log.tree_of(s).filter(|t| t.tree == s.tree)?; // not cached: a later step may have its record
        let mut r = None;
        for (k, f) in self.lib.files().iter().enumerate() {
            if let Some(&id) = f.trees.get(&s.tree)
                && same_shape(self.lib.tree_at(id), &t.raw)
            {
                r = Some((id, FileId(k as u32)));
                break;
            }
        }
        if r.is_none()
            && let Some(want) = env("REPLAY_SHOW_SHAPE")
            && s.tree == atoi(&want) as u16
            && self.shown < 2
        {
            self.shown += 1;
            let mut line = format!("tree {} at {:08x}: {} nodes logged:", s.tree, s.nodes, t.raw.len() / 12);
            for (k, b) in t.raw.iter().enumerate() {
                line += &format!("{}{b:02x}", if k % 12 != 0 { "" } else { " " });
            }
            println!("{line}");
            for f in self.lib.files() {
                let Some(&id) = f.trees.get(&s.tree) else { continue };
                let mut l = format!("  {}:", f.path);
                for n in &self.lib.tree_at(id).nodes {
                    l += &format!(
                        " {:04x} {:02x} {:02x} {:04x}{:04x}{:04x}{:04x}",
                        n.opcode,
                        n.true_target,
                        n.false_target,
                        n.operand16(0),
                        n.operand16(1),
                        n.operand16(2),
                        n.operand16(3)
                    );
                }
                println!("{l}");
            }
        }
        self.cache.insert((s.nodes, s.tree), r);
        r
    }
}
