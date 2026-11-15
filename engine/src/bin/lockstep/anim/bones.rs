//! The skeleton studies (LOCKSTEP_ANIM_BONES): the bone matrices computed natively from the
//! logged final node states (`skeleton::bone_matrices`, EAnimController::ComputeMatrices at
//! 0x80158B60) against the logged ones, and the logged skeletons' character nodes against the
//! skeletons of `characte.arc`.

use crate::*;
use housefire::skeleton::{self, Build, CharNode, Mat4, NodeState, PoseMode};
use housefire::tree_log::{BonesRec, Log};

// A character node's 45 words as the original keeps them (180 bytes). Words 1 and 44 are
// pointers (the children's array and the next node's place), not in the data.
const W_PARENT: usize = 0;
const W_CHILDREN: usize = 2;
const W_CHILD_CAPACITY: usize = 3;
const W_TRANSLATION: usize = 4;
const W_ROTATION: usize = 7;
const W_MATRIX_A: usize = 11;
const W_MATRIX_B: usize = 27;
const W_FLAG: usize = 43;

// A node state's 16 words: the flags, the rotation (4), the translation (3), the scale (3);
// words 11 to 15 are not read by the bones.
const S_TRANSLATION: usize = 5;

fn f(w: u32) -> f32 {
    f32::from_bits(w)
}

fn char_node(w: &[u32; 45]) -> CharNode {
    CharNode {
        parent: w[W_PARENT] as i32,
        translation: [f(w[W_TRANSLATION]), f(w[W_TRANSLATION + 1]), f(w[W_TRANSLATION + 2])],
        rotation: std::array::from_fn(|i| f(w[W_ROTATION + i])),
        rest: std::array::from_fn(|i| f(w[W_MATRIX_B + i])),
        direct: w[W_FLAG] != 0,
    }
}

fn node_state(w: &[u32; 16]) -> NodeState {
    NodeState { flags: w[0], rotation: std::array::from_fn(|i| f(w[1 + i])), translation: std::array::from_fn(|i| f(w[S_TRANSLATION + i])), scale: std::array::from_fn(|i| f(w[8 + i])) }
}

/// What a state holds of the pose before the bones overwrote its translation: the flags, the
/// rotation and the scale.
fn pose_key(states: &[[u32; 16]]) -> Vec<u32> {
    states.iter().flat_map(|w| [w[0], w[1], w[2], w[3], w[4], w[8], w[9], w[10]]).collect()
}

/// The words of a matrix as they are logged.
fn bits(m: &Mat4) -> [u32; 16] {
    m.map(f32::to_bits)
}

/// One way the controller can have scaled and built its matrices.
#[derive(Clone, Copy, Debug)]
struct Setting {
    through_rest: bool,
    scale: f32,
}

/// The matrices for a record under a setting, and how many nodes differ from the logged ones
/// (any of the 16 words), by how they were built.
fn run(nodes: &[CharNode], states: &[NodeState], b: &BonesRec, set: Setting) -> (Vec<Mat4>, BTreeMap<Build, (usize, usize)>) {
    let root: Mat4 = std::array::from_fn(|i| f(b.root[i]));
    let mode = PoseMode { through_rest: set.through_rest, scale: set.scale };
    let m = skeleton::bone_matrices(nodes, states, &root, mode);
    let mut by_build: BTreeMap<Build, (usize, usize)> = BTreeMap::new();
    for (i, mm) in m.iter().enumerate() {
        let e = by_build.entry(skeleton::build(&nodes[i], &states[i], mode)).or_default();
        e.0 += 1;
        e.1 += usize::from(bits(mm) != b.matrices[i]);
    }
    (m, by_build)
}

/// The controller's scale: the logged first matrix's largest 3 x 3 element over what the
/// states give unscaled (the controller's scale is not logged); with its neighbours in the last
/// place, as the division is not exact for every scale.
fn scale_candidates(nodes: &[CharNode], states: &[NodeState], b: &BonesRec) -> Vec<f32> {
    let unit = Setting { through_rest: true, scale: 1.0 };
    let (m, _) = run(nodes, states, b, unit);
    let k = [0usize, 1, 2, 4, 5, 6, 8, 9, 10].into_iter().max_by(|&x, &y| m[0][x].abs().total_cmp(&m[0][y].abs())).unwrap_or(0);
    let est = f(b.matrices[0][k]) / m[0][k];
    let mut out = vec![1.0];
    out.extend((-2i32..=2).map(|d| f32::from_bits((est.to_bits() as i32 + d) as u32)));
    out
}

/// The closest a search came for a record: the pose tried (0 the latest), the nodes wrong, how
/// they were built and the setting.
struct Best {
    rank: usize,
    wrong: usize,
    by_build: BTreeMap<Build, (usize, usize)>,
    set: Setting,
}

pub fn bones(log: &Log, d: &Disc) {
    bone_matrices(log);
    skeletons(log, d);
}

/// Study 1: the bone matrices from the states alone: the controller's mode word and scale are not
/// logged and are found for each record (both modes, the scale from the first matrix); the
/// states' translations, which the original overwrites with the matrices' own, are those of the
/// pose ('=') that gave the states, found by the rest of their words.
fn bone_matrices(log: &Log) {
    let skeletons: HashMap<u32, Vec<CharNode>> = log.skeletons.iter().map(|s| (s.skeleton, s.nodes.iter().map(char_node).collect())).collect();
    let mut poses: HashMap<Vec<u32>, Vec<usize>> = HashMap::new();
    for (i, p) in log.node_states.iter().enumerate() {
        poses.entry(pose_key(&p.after)).or_default().push(i);
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut builds: BTreeMap<Build, (usize, usize)> = BTreeMap::new();
    let mut scales: BTreeMap<u32, usize> = BTreeMap::new();
    let mut different = 0;
    let mut shown = 0;
    for b in &log.bones {
        let Some(nodes) = skeletons.get(&b.skeleton) else {
            *counts.entry("skeleton not logged".into()).or_default() += 1;
            different += 1;
            continue;
        };
        // the pose's translations, the latest pose of these states first
        let found: Vec<&Vec<[u32; 16]>> = poses.get(&pose_key(&b.states)).map(|v| v.iter().rev().filter(|&&i| log.node_states[i].tick <= b.tick).map(|&i| &log.node_states[i].after).collect()).unwrap_or_default();
        let mut best: Option<Best> = None;
        'search: for (rank, after) in found.iter().enumerate() {
            let states: Vec<NodeState> = b.states.iter().zip(after.iter()).map(|(s, a)| node_state(&std::array::from_fn(|k| if (S_TRANSLATION..S_TRANSLATION + 3).contains(&k) { a[k] } else { s[k] }))).collect();
            for scale in scale_candidates(nodes, &states, b) {
                for through_rest in [false, true] {
                    let set = Setting { through_rest, scale };
                    let (_, by_build) = run(nodes, &states, b, set);
                    let wrong: usize = by_build.values().map(|v| v.1).sum();
                    if best.as_ref().is_none_or(|x| wrong < x.wrong) {
                        best = Some(Best { rank, wrong, by_build, set });
                    }
                    if wrong == 0 {
                        break 'search;
                    }
                }
            }
        }
        let Some(Best { rank, wrong, by_build, set }) = best else {
            *counts.entry("no pose found for the states".into()).or_default() += 1;
            different += 1;
            continue;
        };
        for (k, v) in &by_build {
            let e = builds.entry(*k).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
        if wrong == 0 {
            let rest = by_build.contains_key(&Build::ThroughRest);
            // the mode word matters only if some node goes through its rest
            *counts.entry(format!("same ({}; pose {})", if rest { "mode word not 0" } else { "either mode word" }, if rank == 0 { "the latest" } else { "an earlier" })).or_default() += 1;
            *scales.entry(set.scale.to_bits()).or_default() += 1;
            // the logged scale (+84) against the one found
            *counts.entry(if set.scale == b.scale { "the found scale the logged one".into() } else { format!("the found scale not the logged one ({:e} vs {:e})", set.scale, b.scale) }).or_default() += 1;
        } else {
            different += 1;
            *counts.entry("DIFFERENT".into()).or_default() += 1;
            if shown < 5 {
                shown += 1;
                println!("  bones different: tick {} controller {:#x}, {wrong} of {} nodes", b.tick, b.controller, b.states.len());
            }
        }
    }
    println!("bone matrices from the states ({} records, {} nodes in all): DIFFERENT -> {different}; {counts:?}", log.bones.len(), log.bones.iter().map(|b| b.states.len()).sum::<usize>());
    for (k, (n, wrong)) in &builds {
        println!("  built as {k:?}: {n} nodes, {wrong} different");
    }
    let scales: Vec<String> = scales.iter().map(|(s, n)| format!("{:e} x{n}", f(*s))).collect();
    println!("  the controller's scale found: {}", scales.join(", "));
}

/// Study 2: each logged skeleton's character nodes against `characte.arc`'s skeletons, field by field:
/// the parent, the rest translation and rotation, the two matrices, the flag and the child count
/// (words 1 and 44 are pointers).
fn skeletons(log: &Log, d: &Disc) {
    let arc_bytes = d.read("characte.arc").unwrap_or_default();
    let entries = housefire::arc::entries(&arc_bytes).unwrap_or_default();
    let disc: Vec<(String, skeleton::Skeleton)> = entries.iter().filter_map(|e| skeleton::read(housefire::arc::bytes(&arc_bytes, e)).map(|s| (e.name.clone(), s))).collect();
    // how many of a logged skeleton's nodes' fields a disc skeleton has the same
    let fields = |w: &[u32; 45], b: &skeleton::Bone| -> [bool; 7] {
        let eq = |logged: &[u32], disc: &[f32]| logged.iter().zip(disc).all(|(l, d)| *l == d.to_bits());
        [
            w[W_PARENT] as i32 == b.parent,
            eq(&w[W_TRANSLATION..W_TRANSLATION + 3], &b.translation),
            eq(&w[W_ROTATION..W_ROTATION + 4], &b.rotation),
            eq(&w[W_MATRIX_A..W_MATRIX_A + 16], &b.matrices[0]),
            eq(&w[W_MATRIX_B..W_MATRIX_B + 16], &b.matrices[1]),
            w[W_FLAG] == u32::from(b.flag),
            w[W_CHILDREN] as usize == b.children.len(),
        ]
    };
    const NAMES: [&str; 7] = ["parent", "translation", "rotation", "matrix 1", "matrix 2", "flag", "child count"];
    let (mut found, mut different, mut capacity_differs) = (0, 0, 0);
    let mut wrong_fields = [0usize; 7];
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for sk in &log.skeletons {
        let n = sk.nodes.len();
        // the disc skeleton with the most equal fields over its nodes
        let score = |s: &skeleton::Skeleton| -> usize { sk.nodes.iter().zip(&s.bones).map(|(w, b)| fields(w, b).iter().filter(|x| **x).count()).sum() };
        let best = disc.iter().filter(|(_, s)| s.bones.len() == n).max_by_key(|(_, s)| score(s));
        capacity_differs += sk.nodes.iter().filter(|w| w[W_CHILD_CAPACITY] != w[W_CHILDREN]).count();
        let Some((name, s)) = best else {
            different += 1;
            println!("  skeleton {:#x} ({n} nodes): no disc skeleton of that size", sk.skeleton);
            continue;
        };
        let mut wrong = [0usize; 7];
        for (w, b) in sk.nodes.iter().zip(&s.bones) {
            for (k, ok) in fields(w, b).iter().enumerate() {
                wrong[k] += usize::from(!ok);
            }
        }
        if wrong.iter().all(|w| *w == 0) {
            found += 1;
            *used.entry(name.clone()).or_default() += 1;
        } else {
            different += 1;
            for k in 0..7 {
                wrong_fields[k] += wrong[k];
            }
            println!("  skeleton {:#x} ({n} nodes): closest {name}, nodes with a field different: {:?}", sk.skeleton, NAMES.iter().zip(wrong).filter(|(_, w)| *w > 0).collect::<Vec<_>>());
        }
    }
    println!(
        "skeletons from the disc ({} logged, {} on the disc): found {found}; DIFFERENT -> {different} (fields: parent, rest translation, rest rotation, both matrices, flag, child count; words 1 and 44 are addresses; the child array's capacity word differs from the count in {capacity_differs} nodes)",
        log.skeletons.len(),
        disc.len()
    );
    let names: Vec<String> = used.iter().map(|(n, c)| if *c > 1 { format!("{n} x{c}") } else { n.clone() }).collect();
    println!("  {}", names.join(", "));
}
