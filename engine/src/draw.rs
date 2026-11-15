//! Which model part a draw is: a strip of short arrays the device played names its positions by
//! address, and the same bytes are in `models.arc`'s records (`docs/assets.md`, "Models"). The
//! part found says the bone it hangs on (its node's word, `model::Node::bone`).

use std::collections::HashMap;

use crate::arc;
use crate::model::{self, Node, Record, SubModel};
use crate::tree_log::StripRec;

/// How many bytes of an array the log keeps at each pointer of a strip (`StripRec::at`).
pub const KEPT: usize = 32;

/// A part of a model: a batch of one node of a record.
#[derive(Clone, Copy, Debug)]
pub struct Part<'a> {
    pub record: &'a Record,
    pub node: &'a Node,
    pub batch: &'a SubModel,
}

impl Part<'_> {
    /// The bone of the controller's skeleton whose matrix is the part's world matrix: a node with
    /// a bone word (0 or more) hangs on it; a node without (-1) names its matrices in its batches
    /// (`01 00 slot index`) and the batch's first slot is the bone (slot s, bone s). `None` for a
    /// batch with no slot (an object's one-piece mesh): it is drawn with the model's own matrix,
    /// which is a bone's (bone 0) only where the model has a controller (a deckchair's one-bone
    /// skeleton), else its placement alone.
    pub fn bone(&self) -> Option<usize> {
        match usize::try_from(self.node.bone) {
            Ok(bone) => Some(bone),
            Err(_) => self.batch.matrices.first().map(|&(slot, _)| usize::from(slot)),
        }
    }

    /// Whether the batch blends several matrices (weights, 3 or 4 slots) rather than hangs on one.
    pub fn skinned(&self) -> bool {
        self.batch.matrices.len() > 1
    }
}

/// The parts of `models.arc`'s records, found by their positions' first bytes.
pub struct PartIndex {
    records: Vec<Record>,
    /// The first 8 bytes of each batch's positions -> (record, node, batch's place in the node).
    by_head: HashMap<[u8; 8], Vec<(usize, usize, usize)>>,
}

impl PartIndex {
    /// Reads every record of `models.arc` that reads whole.
    pub fn new(models_arc: &[u8]) -> PartIndex {
        let records: Vec<Record> = arc::entries(models_arc).unwrap_or_default().iter().filter_map(|e| model::read_record(arc::bytes(models_arc, e))).collect();
        let mut by_head: HashMap<[u8; 8], Vec<(usize, usize, usize)>> = HashMap::new();
        for (r, rec) in records.iter().enumerate() {
            for (n, node) in rec.nodes.iter().enumerate() {
                for (s, (_, sub)) in node.subs.iter().enumerate() {
                    if let Some(head) = sub.positions.get(..8) {
                        by_head.entry(head.try_into().unwrap()).or_default().push((r, n, s));
                    }
                }
            }
        }
        PartIndex { records, by_head }
    }

    /// The parts whose positions start with a strip's first array bytes (the log's `KEPT` bytes,
    /// compared as far as the batch has any) and whose vertex count is the strip's.
    pub fn find(&self, strip: &StripRec) -> Vec<Part<'_>> {
        let (positions, count) = (&strip.at[2], strip.words[1]);
        let head: [u8; 8] = positions[..8].try_into().unwrap();
        let Some(hits) = self.by_head.get(&head) else { return Vec::new() };
        let mut found: Vec<(usize, Part<'_>)> = hits
            .iter()
            .map(|&(r, n, s)| {
                let record = &self.records[r];
                let node = &record.nodes[n];
                Part { record, node, batch: &node.subs[s].1 }
            })
            .filter(|p| p.batch.count as u32 == count && starts_with(&p.batch.positions, positions))
            .map(|p| (p.arrays_in(strip), p))
            .collect();
        let best = found.iter().map(|f| f.0).max().unwrap_or(0);
        found.retain(|f| f.0 == best);
        found.into_iter().map(|f| f.1).collect()
    }
}

/// Whether `array` starts with `kept` (the strip's bytes at an array's pointer), as far as the
/// array goes.
fn starts_with(array: &[u8], kept: &[u8; KEPT]) -> bool {
    array.iter().zip(kept).all(|(a, b)| a == b)
}

impl Part<'_> {
    /// How many of the strip's arrays (after the positions) start the same as one of the batch's.
    fn arrays_in(&self, strip: &StripRec) -> usize {
        let b = self.batch;
        let own = [Some(&b.texcoords), b.colours.as_ref(), Some(&b.normals), Some(&b.blend)];
        (3..strip.words.len())
            .filter(|&k| strip.at[k] != [0u8; KEPT] && own.iter().flatten().any(|a| !a.is_empty() && starts_with(a, &strip.at[k])))
            .count()
    }
}
