//! Choosing the disc or save lot a log ran on.

use crate::library::Library;
use crate::save::{read_lots, LotChunks};
use crate::tree_log::{Log, StepRec};
use crate::world::World;
use std::collections::BTreeSet;


/// The lot the log ran on, among the save's (or the disc's) lots: the one whose table order the
/// original's longest walk over every object follows furthest; with the lots, their ids, the walk
/// and how far it follows.
pub type LotPick = (Vec<LotChunks>, Vec<BTreeSet<i16>>, Vec<i16>, usize, usize);
pub fn pick_lot(lib: &Library, ngh: &[u8], log: &Log) -> LotPick {
    let lots = read_lots(ngh).expect("the lots");
    let orders: Vec<Vec<i16>> = lots.iter().map(|l| World::new(lib).read_objects(l).unwrap().objects.iter().map(|o| o.id).collect()).collect();
    let lot_ids: Vec<BTreeSet<i16>> = orders.iter().map(|o| o.iter().copied().collect()).collect();
    // Which lot: the one whose table order the original's longest walk over every object
    // (SetToNext kind 0 from 0 on the stack object) follows furthest. Object ids are small
    // numbers every lot reuses, so they do not tell lots apart; the order does.
    let (mut walk, mut cur): (Vec<i16>, Vec<i16>) = (Vec::new(), Vec::new());
    let mut walking = false;
    let raw_of = |s: &StepRec| -> Option<&[u8]> {
        let t = log.tree_of(s)?;
        if s.look != b'X' || t.tree != s.tree || s.node as usize * 12 >= t.raw.len() {
            return None;
        }
        Some(&t.raw[12 * s.node as usize..12 * s.node as usize + 12])
    };
    for s in &log.steps {
        let Some(raw) = raw_of(s) else { continue };
        if (u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF) != 31 || (raw[8] & 0x7F) != 0 || raw[9] != 0x0A || raw[2] == raw[3] {
            continue;
        }
        if s.stack_object == 0 {
            cur.clear();
            walking = true;
        }
        if !walking {
            continue;
        }
        if s.after == raw[2] as u16 {
            cur.push(s.stack_object_after);
            if cur.len() > walk.len() {
                walk = cur.clone();
            }
        } else {
            walking = false;
        }
    }
    let (mut best, mut best_n) = (0, 0);
    for k in 0..lots.len() {
        let mut n = 0;
        for id in &walk {
            if !lot_ids[k].contains(id) {
                continue;
            }
            if n < orders[k].len() && orders[k][n] == *id {
                n += 1;
            } else {
                break;
            }
        }
        if n > best_n {
            best = k;
            best_n = n;
        }
    }
    (lots, lot_ids, walk, best, best_n)
}
