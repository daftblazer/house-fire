//! Autonomy: what a Sim with nothing to do considers and chooses (`docs/simulation.md`,
//! "Autonomy"). Measured on the recompiled Bustin' Out's home log (its three evaluations).

use crate::interp::Host;
use crate::library::{Advert, Interaction, Library};
use crate::world::{Object, ObjectKey, World};
use crate::{FileId, TreeId};

/// One interaction considered: the object, and its interaction (in the table `file`'s TTAB
/// `table` names, at `index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub object: ObjectKey,
    pub id: i16,
    pub file: FileId,
    pub table: u16,
    pub index: usize,
    /// What its checks need of the object (taken from the world, which the host holds while
    /// they run): its file, whether a person, its definition.
    pub object_file: Option<FileId>,
    pub person: bool,
    pub def: Option<crate::library::DefId>,
}

impl Candidate {
    pub fn interaction<'l>(&self, lib: &'l Library) -> &'l Interaction {
        &lib.table(self.file, self.table)[self.index]
    }
}

/// The interactions an evaluation considers, in the order it checks their guards: the lot's
/// objects by id (not of definition type 7, nor a part whose definition's field 6 is 1 to
/// 0xFFFD), each one's table (`Library::interactions_of`) in reverse order, an interaction
/// whose autonomy threshold is below 100 and whose flags lack 0x80 and 0x40, a guard several
/// share once. A person's table is its semi-global file's (person_sg, NPC_SG). Measured: the
/// home log's three evaluations, objects and persons, the same.
pub fn autonomy_candidates(world: &World, me: ObjectKey) -> Vec<Candidate> {
    let lib = world.library();
    let mut out = Vec::new();
    for (&id, &k) in world.by_id() {
        let o = world.get(k);
        if k == me {
            continue;
        }
        let Some(file) = o.file else { continue };
        let table = if o.person {
            // A person's interactions are its semi-global file's one table: person_sg's for a Sim
            // (whose file is template_person), NPC_SG's for an NPC (the paper girl).
            lib.file(file).semi.and_then(|s| {
                let t = &lib.file(s).tables;
                (t.len() == 1).then(|| (s, *t.keys().next().unwrap()))
            })
        } else {
            let Some(def) = world.definition(o) else { continue };
            // A definition of type 7 (field 7: the phone's and the stereo's controllers, kept off
            // the lot) is not considered; an off-lot one of type 4 is (Mom's house's scenario).
            if def.field(7) == 7 {
                continue;
            }
            // A part whose definition's field 6 is 1 to 0xFFFD (a bed's, a mailbox's lesser
            // parts) is not considered; 0, 0xFFFE and 0xFFFF are.
            if (1..=0xFFFD).contains(&def.field(6)) {
                continue;
            }
            lib.interactions_of(def, file).map(|t| (file, t))
        };
        let Some((tf, tid)) = table else { continue };
        let mut guards: Vec<u16> = Vec::new();
        for (index, e) in lib.table(tf, tid).iter().enumerate().rev() {
            if e.autonomy_threshold >= 100 || e.flags & 0xC0 != 0 || e.guard == 0 || guards.contains(&e.guard) {
                continue;
            }
            guards.push(e.guard);
            out.push(Candidate { object: k, id, file: tf, table: tid, index, object_file: o.file, person: o.person, def: o.definition });
        }
    }
    out
}

/// One check of an evaluation: the object, the tree (a table's first tree, or a candidate's
/// guard; `candidate` `None` for a first tree), and its result (`None` if it did not finish).
#[derive(Clone, Debug)]
pub struct EvaluationCheck {
    pub object: ObjectKey,
    pub tree: u16,
    pub candidate: Option<usize>,
    pub passed: Option<bool>,
    /// The check tree ad range after a candidate's guard (scope 12, a motive's advertisement
    /// delta by motive), as the score then reads it; empty if the host keeps none.
    pub ad_range: Vec<i16>,
}

/// The evaluation (opcode 3, run by the Sims' "try autonomy", PersonGlobals 8233): every
/// candidate's checks through the host (`Host::run_check`, the stack object the candidate, the
/// evaluating stack's own parameters `defaults`), an object's table's first tree once before
/// its guards. Measured: the home log's three evaluations (`docs/simulation.md`, "Autonomy").
pub fn run_evaluation<H: Host + ?Sized>(lib: &Library, host: &mut H, candidates: &[Candidate], defaults: &[i16]) -> Vec<EvaluationCheck> {
    let mut out = Vec::new();
    let mut last: Option<ObjectKey> = None;
    let run = |host: &mut H, t: Option<TreeId>, c: &Candidate| t.and_then(|t| host.run_check(t, c.object_file, c.id, defaults));
    for (k, c) in candidates.iter().enumerate() {
        if last != Some(c.object) {
            // a new object: its table's first tree, once
            last = Some(c.object);
            let tab = lib.table_of(c.object_file, c.person, c.def.map(|d| lib.def(d)));
            if tab.first != 0 {
                let t = lib.tree(c.object_file, tab.first);
                out.push(EvaluationCheck { object: c.object, tree: tab.first, candidate: None, passed: run(host, t, c), ad_range: Vec::new() });
            }
        }
        // The trees as the object's own file names them (a person's table is its semi-global
        // file's, whose ids the object's file resolves).
        let e = c.interaction(lib);
        let guard = e.guard;
        let g = lib.tree(c.object_file, guard);
        // its guard starts with the ad range its interaction's advertisement (each motive's
        // delta) and may add to it (a social interaction's appeal); what it leaves is scored
        let kept = host.ad_range_mut().map(|r| {
            let old = r.clone();
            r.clear();
            r.extend((0..16).map(|m| e.adverts.get(m).map_or(0, |a| a.delta)));
            old
        });
        let passed = run(host, g, c);
        let ad_range = match (kept, host.ad_range_mut()) {
            (Some(old), Some(r)) => std::mem::replace(r, old),
            _ => Vec::new(),
        };
        out.push(EvaluationCheck { object: c.object, tree: guard, candidate: Some(k), passed, ad_range });
    }
    out
}

/// The interaction score's curves (Bustin' Out's fn_80267E9C; `docs/simulation.md`, "The
/// curves"): one a motive, its points (x, y) by x ascending and each segment's 1 / width.
#[derive(Clone, Debug, Default)]
pub struct ScoreCurve {
    pub motive: usize,
    pub points: Vec<(f32, f32)>,
    pub slopes: Vec<f32>,
}

impl ScoreCurve {
    pub fn at(&self, v: f32) -> f32 {
        // The segment from the end: the last point at or below v.
        let n = self.points.len();
        let mut i = n as i64 - 1;
        while i >= 0 && v - self.points[i as usize].0 < 0.0 {
            i -= 1;
        }
        if i < 0 {
            return self.points[0].1;
        }
        let i = i as usize;
        if i == n - 1 {
            return self.points[n - 1].1;
        }
        let (x, y) = self.points[i];
        let y1 = self.points[i + 1].1;
        ((v - x) * self.slopes[i]).mul_add(y1 - y, y)
    }
}

/// From the global file's STR# 501, "InteractionScoreCurves": "(x;y) (x;y) ...", its strings
/// for motives 5, 6, 7, 8, 9, 3, 13, 14 and 15 in turn (the tenth is not used here).
pub fn score_curves(lib: &Library) -> Vec<ScoreCurve> {
    const MOTIVES: [usize; 9] = [5, 6, 7, 8, 9, 3, 13, 14, 15];
    let mut out = Vec::new();
    let Some(g) = lib.global_file() else { return out };
    let Some(t) = g.strings.get(&501) else { return out };
    for (k, &motive) in MOTIVES.iter().enumerate().take(t.len()) {
        let mut c = ScoreCurve { motive, ..Default::default() };
        let s = &t[k];
        for (p, _) in s.match_indices('(') {
            if let Some(pt) = scan_point(&s[p..]) {
                c.points.push(pt);
            }
        }
        c.points.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for i in 0..c.points.len().saturating_sub(1) {
            c.slopes.push(1.0 / (c.points[i + 1].0 - c.points[i].0));
        }
        if !c.points.is_empty() {
            out.push(c);
        }
    }
    out
}

/// "(x;y" as sscanf's "(%f;%f)" reads it: two floats, the second after a ';'.
fn scan_point(s: &str) -> Option<(f32, f32)> {
    let s = s.strip_prefix('(')?;
    let (x, rest) = scan_float(s)?;
    let rest = rest.strip_prefix(';')?;
    let (y, _) = scan_float(rest)?;
    Some((x, y))
}

/// A float as strtof reads one: blanks, a sign, digits, a point, digits, an exponent.
fn scan_float(s: &str) -> Option<(f32, &str)> {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start_digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start_digits || (i == start_digits + 1 && b[start_digits] == b'.') {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    Some((t[..i].parse().ok()?, &t[i..]))
}

/// An advert's amount for a person (its minimum, plus the delta scaled by the personality field
/// it names, or 1000 less it), as the score takes it.
pub fn advert_amount(a: &Advert, person: &Object) -> f32 {
    // The personality table (0x802D17F8, 12 bytes an entry): entry p's person data field is
    // p / 2 + 1, the odd entries counting 1000 less it (1: field 2; 2: field 2, inverted; ...).
    if a.personality == 0 {
        return (a.min as i32 + a.delta as i32) as f32;
    }
    let field = (a.personality as usize).div_ceil(2) + 1;
    let inverted = a.personality.is_multiple_of(2);
    let mut v = person.person_data.get(field).copied().unwrap_or(0) as i32;
    if inverted {
        v = 1000 - v;
    }
    a.min as f32 + (a.delta as i32 * v) as f32 / 1000.0
}

/// The score of a set of adverts (one a motive; `None`: none) for a person with these motives.
pub fn interaction_score(curves: &[ScoreCurve], adverts: Option<&[Advert]>, person: &Object, motives: &[f32]) -> f32 {
    if curves.is_empty() {
        return 0.0;
    }
    let mut total = 0.0f32;
    for c in curves {
        let ad = adverts.and_then(|a| a.get(c.motive)).map_or(0.0, |a| advert_amount(a, person));
        let m = motives.get(c.motive).copied().unwrap_or(0.0);
        total += c.at(ad.mul_add(0.001, m));
    }
    total / curves.len() as f32
}

/// The game's square root (fn_801F1BB8): a first guess of 1 + s / 20, then two of Heron's
/// steps; up to 4% over near s = 25, which the choices' values show (`docs/simulation.md`).
pub fn game_sqrt(s: f32) -> f32 {
    if s <= 0.0 {
        return 0.0;
    }
    let mut x = s.mul_add(0.05, 1.0);
    x = (x + s / x) * 0.5;
    x = (x + s / x) * 0.5;
    x
}

/// A candidate's distance in tiles (fn_801F1C18: from the two places, 20 a level).
pub fn person_distance(a: &Object, b: &Object) -> f32 {
    let (dx, dy) = (a.x16.wrapping_sub(b.x16), a.y16.wrapping_sub(b.y16));
    let flat = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy)) as f32 * 0.00390625; // 1/256: sixteenths to tiles, squared
    let up = (a.level.wrapping_sub(b.level).wrapping_mul(20)) as f32;
    game_sqrt(up.mul_add(up, flat))
}

/// The attenuation by the interaction's code: 1 none, 2, 3, 4 the tuning table's low, moderate
/// and high (FCNS 2), 0 the interaction's own value.
pub fn attenuation(lib: &Library, e: &Interaction, visitor: bool) -> f32 {
    let t = |name: &str| lib.tuning(name).unwrap_or(0.0);
    match e.attenuation_code {
        0 => e.attenuation_value,
        2 => t(if visitor { "visitor low attenuation" } else { "low attenuation" }),
        3 => t(if visitor { "visitor moderate attenuation" } else { "moderate attenuation" }),
        4 => t(if visitor { "visitor high attenuation" } else { "high attenuation" }),
        _ => 0.0,
    }
}

/// A candidate's value to a person (fn_802266BC): its score less the base (the score of no
/// advert), over 1 + the attenuation × the distance in tiles.
pub fn candidate_value(lib: &Library, curves: &[ScoreCurve], person: &Object, object: &Object, c: &Candidate, adverts: &[crate::library::Advert], base: f32) -> f32 {
    let e = c.interaction(lib);
    let score = interaction_score(curves, Some(adverts), person, &person.motives);
    let factor = 1.0 / person_distance(person, object).mul_add(attenuation(lib, e, false), 1.0);
    (score - base) * factor
}

/// An entry of the list a choice is made from: a value and its candidate (its place in the
/// candidates).
#[derive(Clone, Copy, Debug)]
pub struct Valued {
    pub value: f32,
    pub candidate: usize,
}

/// The list a choice is made from (fn_802266BC): the passing candidates whose value reaches the
/// tuning's minimum (FCNS 2, "min autonomy score for family" or "... for visitors"), best first.
/// `values` come in the candidates' order; the original's list holds them the other way round
/// (last candidate first), which is the order its sort starts from: values within 1e-7 end in
/// that order (measured: object 124's two equal entries at the home log's tick 1443).
pub fn autonomy_list(lib: &Library, mut values: Vec<Valued>, visitor: bool) -> Vec<Valued> {
    let least = lib.tuning(if visitor { "min autonomy score for visitors" } else { "min autonomy score for family" }).unwrap_or(0.0);
    values.reverse();
    values.retain(|v| v.value >= least);
    // A stable sort with the original's comparator (8022667C): ahead when better by more than
    // 1e-7. It is not an order (ties do not chain), so the sort is the C++'s, an insertion of
    // each in turn after those it is not ahead of.
    stable_sort_by(&mut values, |a, b| b.value - a.value < -1e-7);
    values
}

/// `std::stable_sort` as libstdc++ (GCC 13 on) runs it, with its buffer of half the length:
/// the two halves each merge-sorted (chunks of 7 sorted by insertion, then merged in turn into
/// the buffer and back), then merged, forwards or (an odd length) backwards. With a comparator
/// that is not a strict order (the original's near-ties) the result depends on the algorithm,
/// so it is the one the C++ engine ran. `less(a, b)`: a goes before b.
fn stable_sort_by<T: Copy>(v: &mut [T], less: impl Fn(&T, &T) -> bool) {
    let n = v.len();
    if n == 0 {
        return;
    }
    let middle = n.div_ceil(2);
    let mut buffer: Vec<T> = v[..middle].to_vec();
    merge_sort_with_buffer(&mut v[..middle], &mut buffer, &less);
    merge_sort_with_buffer(&mut v[middle..], &mut buffer, &less);
    let (len1, len2) = (middle, n - middle);
    if len1 <= len2 {
        buffer[..len1].copy_from_slice(&v[..middle]);
        // __move_merge_adaptive: the buffer's (the first half) and the second half, forwards.
        let (mut i, mut j, mut r) = (0, middle, 0);
        while i < len1 && j < n {
            if less(&v[j], &buffer[i]) {
                v[r] = v[j];
                j += 1;
            } else {
                v[r] = buffer[i];
                i += 1;
            }
            r += 1;
        }
        while i < len1 {
            v[r] = buffer[i];
            i += 1;
            r += 1;
        }
    } else {
        buffer[..len2].copy_from_slice(&v[middle..]);
        // __move_merge_adaptive_backward: the first half in place and the buffer's (the
        // second half), from the back.
        let mut result = n;
        if len2 == 0 {
            return;
        }
        let (mut l1, mut l2) = (middle - 1, len2 - 1);
        loop {
            if less(&buffer[l2], &v[l1]) {
                result -= 1;
                v[result] = v[l1];
                if l1 == 0 {
                    v[result - (l2 + 1)..result].copy_from_slice(&buffer[..=l2]);
                    return;
                }
                l1 -= 1;
            } else {
                result -= 1;
                v[result] = buffer[l2];
                if l2 == 0 {
                    return;
                }
                l2 -= 1;
            }
        }
    }
}

fn insertion_sort<T: Copy>(v: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    for i in 1..v.len() {
        let x = v[i];
        if less(&x, &v[0]) {
            v.copy_within(0..i, 1);
            v[0] = x;
        } else {
            let mut j = i;
            while less(&x, &v[j - 1]) {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = x;
        }
    }
}

/// __move_merge: two sorted runs into `out`, the second's element first only when it goes
/// before.
fn move_merge<T: Copy>(a: &[T], b: &[T], out: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    let (mut i, mut j, mut r) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if less(&b[j], &a[i]) {
            out[r] = b[j];
            j += 1;
        } else {
            out[r] = a[i];
            i += 1;
        }
        r += 1;
    }
    out[r..r + a.len() - i].copy_from_slice(&a[i..]);
    r += a.len() - i;
    out[r..r + b.len() - j].copy_from_slice(&b[j..]);
}

/// __merge_sort_loop: runs of `step` merged pairwise from `from` into `to`.
fn merge_sort_loop<T: Copy>(from: &[T], to: &mut [T], step: usize, less: &impl Fn(&T, &T) -> bool) {
    let n = from.len();
    let mut f = 0;
    while n - f >= 2 * step {
        move_merge(&from[f..f + step], &from[f + step..f + 2 * step], &mut to[f..f + 2 * step], less);
        f += 2 * step;
    }
    let s = step.min(n - f);
    move_merge(&from[f..f + s], &from[f + s..n], &mut to[f..n], less);
}

/// __merge_sort_with_buffer.
fn merge_sort_with_buffer<T: Copy>(v: &mut [T], buffer: &mut Vec<T>, less: &impl Fn(&T, &T) -> bool) {
    const CHUNK: usize = 7;
    let n = v.len();
    let mut f = 0;
    while n - f >= CHUNK {
        insertion_sort(&mut v[f..f + CHUNK], less);
        f += CHUNK;
    }
    insertion_sort(&mut v[f..], less);
    buffer.resize(buffer.len().max(n), v.first().copied().unwrap_or_else(|| buffer[0]));
    let mut step = CHUNK;
    while step < n {
        merge_sort_loop(v, &mut buffer[..n], step, less);
        step *= 2;
        let b: Vec<T> = buffer[..n].to_vec();
        merge_sort_loop(&b, v, step, less);
        step *= 2;
    }
}

/// The choice's place in that list: one of the best "random selection count" (4) by a random
/// number (its remainder by that count, or by the list's length if shorter). `None` when the
/// list is empty. The original takes the best instead for a visitor's evaluation and when the
/// best entry has a flag of its interaction's set (not identified: none in the home log), and
/// none for a sitting Sim below "min autonomy score for sitting"; not built, as the log has
/// none of them. Measured: the home log's three choices, from the original's own random numbers
/// ('R'), the same as the original's.
pub fn autonomy_pick(lib: &Library, list: &[Valued], random: u32) -> Option<usize> {
    if list.is_empty() {
        return None;
    }
    let count = lib.tuning("random selection count").unwrap_or(1.0) as u32;
    let k = count.max(1).min(list.len() as u32);
    Some((random % k) as usize)
}
