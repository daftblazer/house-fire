//! Skeletons: `characte.arc`'s entries (`docs/assets.md`, "Skeletons"). An entry is its name,
//! a bone count, then the bones (the first named by the entry's name, the others each by its
//! own): the parent (-1 for the root), the children (a count, then their indices), a translation
//! (three floats), a rotation (a quaternion, four), a flag byte and two 4x4 matrices; then a
//! plane's name and five floats.

fn be32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

fn f32s<const N: usize>(b: &[u8], p: usize) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for (k, v) in out.iter_mut().enumerate() {
        *v = f32::from_bits(be32(b, p + 4 * k)?);
    }
    Some(out)
}

fn cstr(b: &[u8], p: usize) -> Option<(String, usize)> {
    let len = b.get(p..)?.iter().position(|&c| c == 0)?;
    Some((b[p..p + len].iter().map(|&c| c as char).collect(), p + len + 1))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    pub children: Vec<u32>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub flag: u8,
    pub matrices: [[f32; 16]; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    pub name: String,
    pub bones: Vec<Bone>,
    pub plane: (String, [f32; 5]),
}

/// A skeleton, read to its end; `None` unless every byte is accounted for.
pub fn read(b: &[u8]) -> Option<Skeleton> {
    let (name, mut p) = cstr(b, 0)?;
    let count = be32(b, p)? as usize;
    p += 4;
    if count > b.len() / 150 + 1 {
        return None;
    }
    let mut bones = Vec::with_capacity(count);
    for k in 0..count {
        let bone_name = if k == 0 {
            name.clone()
        } else {
            let (n, q) = cstr(b, p)?;
            p = q;
            n
        };
        let parent = be32(b, p)? as i32;
        let n = be32(b, p + 4)? as usize;
        p += 8;
        if n > count {
            return None;
        }
        let children = (0..n).map(|j| be32(b, p + 4 * j)).collect::<Option<Vec<u32>>>()?;
        p += 4 * n;
        let translation = f32s::<3>(b, p)?;
        let rotation = f32s::<4>(b, p + 12)?;
        let flag = *b.get(p + 28)?;
        p += 29;
        let matrices = [f32s::<16>(b, p)?, f32s::<16>(b, p + 64)?];
        p += 128;
        bones.push(Bone { name: bone_name, parent, children, translation, rotation, flag, matrices });
    }
    let (plane_name, q) = cstr(b, p)?;
    let plane = (plane_name, f32s::<5>(b, q)?);
    (q + 20 == b.len()).then_some(Skeleton { name, bones, plane })
}

// ---------------------------------------------------------------------------------------------
// Posing: the bone matrices from the nodes' final states (EAnimController::ComputeMatrices,
// 0x80158B60). Matrices are 4x4, row-major, for row vectors (the translation in the last row);
// every step is rounded to single, the fused steps fused as the original's FPU does them.

/// A 4x4 matrix, row by row; the translation is the fourth row.
pub type Mat4 = [f32; 16];

/// The identity.
pub const IDENTITY: Mat4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

/// `a * b + c` as the original's fmadds do it: the product exact, the sum rounded in double and
/// then to single.
fn madd(a: f32, b: f32, c: f32) -> f32 {
    f64::from(a).mul_add(f64::from(b), f64::from(c)) as f32
}

/// The bit of a node state's flag word that says the node is posed at all (0x80158C5C); without
/// it the node's matrix is its parent's.
pub const POSED: u32 = 1;
/// A posed node whose state's scale is applied (0x80158C68; without it the rotation is used as
/// it is).
pub const SCALED: u32 = 2;
/// A scaled node built through its rest matrix and rest rotation, when the controller allows
/// it and the node has no `direct` word (0x80158C70).
pub const THROUGH_REST: u32 = 4;

/// What the skeleton gives a bone's pose: where it hangs and its rest placement. Read from the
/// loaded character node (180 bytes, 45 words in the original).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharNode {
    /// The bone it hangs from (-1 for the root).
    pub parent: i32,
    /// Its rest translation.
    pub translation: [f32; 3],
    /// Its rest rotation, a quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    /// Its rest matrix.
    pub rest: Mat4,
    /// Whether the node is posed from its state alone even when its flags ask for its rest
    /// matrix (the word after the rest matrix is not 0, 0x80158C78).
    pub direct: bool,
}

impl CharNode {
    /// From a bone of `characte.arc`'s skeleton: the rest translation and rotation, the second
    /// matrix the rest matrix (the first, the rest rotation as a matrix, is not used here), and
    /// the flag byte the `direct` word.
    pub fn from_bone(b: &Bone) -> CharNode {
        CharNode { parent: b.parent, translation: b.translation, rotation: b.rotation, rest: b.matrices[1], direct: b.flag != 0 }
    }
}

/// A node's final pose state (64 bytes in the original): the flag word, a rotation, a
/// translation and a scale; the last five words are not read here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeState {
    pub flags: u32,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

/// The quaternion (x, y, z, w) as a rotation matrix (0x801B48D0).
pub fn quaternion_matrix(q: [f32; 4]) -> Mat4 {
    let [a, b, c, d] = q;
    let (a2, b2, c2) = (a + a, b + b, c + c);
    let aa = a2 * a;
    let bb = b2 * b;
    let cc = c2 * c;
    let (cd, ad, ac, bd, bc, ab) = (c2 * d, a2 * d, c2 * a, b2 * d, c2 * b, b2 * a);
    [
        1. - (bb + cc),
        ab + cd,
        ac - bd,
        0.,
        ab - cd,
        1. - (aa + cc),
        bc + ad,
        0.,
        ac + bd,
        bc - ad,
        1. - (aa + bb),
        0.,
        0.,
        0.,
        0.,
        1.,
    ]
}

/// The first three rows of a matrix scaled by one factor each (0x801B28F0).
pub fn scale_rows(m: &mut Mat4, s: [f32; 3]) {
    for (row, k) in s.iter().enumerate() {
        for c in 0..3 {
            m[row * 4 + c] *= k;
        }
    }
}

/// The matrix preceded by a translation of `v`: `v` taken through the 3 x 3 and added to the
/// translation (0x801B2988).
pub fn pre_translate(m: &mut Mat4, v: [f32; 3]) {
    for j in 0..3 {
        let e = v[1] * m[4 + j];
        let e = madd(v[0], m[j], e);
        let e = madd(v[2], m[8 + j], e);
        m[12 + j] += e;
    }
}

/// The matrix's translation moved by `v` (0x801B345C).
pub fn translate(m: &mut Mat4, v: [f32; 3]) {
    for (j, d) in v.iter().enumerate() {
        m[12 + j] += d;
    }
}

/// `a` then `b`, the row-vector product: each element a's row against b's column, the terms
/// added in order and fused from the second (PSMTX44Concat 0x8027988C, which 0x801B2888 calls).
pub fn concat(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.; 16];
    for i in 0..4 {
        for j in 0..4 {
            let mut e = b[j] * a[i * 4];
            for k in 1..4 {
                e = madd(b[k * 4 + j], a[i * 4 + k], e);
            }
            out[i * 4 + j] = e;
        }
    }
    out
}

/// A rest rotation turned by the pose's: the quaternion product, written out as the original
/// does it (0x80158CC4..0x80158D48).
fn compose(s: [f32; 4], r: [f32; 4]) -> [f32; 4] {
    let [s0, s1, s2, s3] = s;
    let [r0, r1, r2, r3] = r;
    let x = madd(-s1, r0, s0 * r1);
    let y = madd(-s0, r0, -(s1 * r1));
    let z = madd(s3, r0, -(s2 * r1));
    let m4 = s2 * r2;
    let z = madd(s1, r2, z);
    let w = madd(s2, r0, s3 * r1);
    let m5 = s0 * r2;
    let y = y - m4;
    let x = madd(s3, r2, x);
    let w = w - m5;
    [madd(s0, r3, z), madd(s1, r3, w), madd(s2, r3, x), madd(s3, r3, y)]
}

/// What the controller adds to its nodes' states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseMode {
    /// The controller's mode word (+96) is not 0: a scaled node with `THROUGH_REST` and no
    /// `direct` word is built through its rest matrix and rest rotation.
    pub through_rest: bool,
    /// The controller's scale (+84): the matrices' 3 x 3 are scaled by it unless it is 1.
    pub scale: f32,
}

/// How a node's matrix is built from its state (0x80158C5C..).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Build {
    /// Not posed: its parent's matrix as it is.
    Inherit,
    /// The state's rotation, unscaled, about the rest translation.
    Turned,
    /// The state's rotation and scale, about the rest translation.
    TurnedScaled,
    /// Through the rest matrix: the pose's rotation by the rest's, scaled.
    ThroughRest,
}

/// How a node is built, from its flags, its rest and the controller.
pub fn build(node: &CharNode, st: &NodeState, mode: PoseMode) -> Build {
    if st.flags & POSED == 0 {
        Build::Inherit
    } else if st.flags & SCALED == 0 {
        Build::Turned
    } else if mode.through_rest && st.flags & THROUGH_REST != 0 && !node.direct {
        Build::ThroughRest
    } else {
        Build::TurnedScaled
    }
}

/// The node's own matrix, before its parent's.
fn local(node: &CharNode, st: &NodeState, how: Build) -> Mat4 {
    let t = node.translation;
    let back = [t[0] + st.translation[0], t[1] + st.translation[1], t[2] + st.translation[2]];
    let minus = [-t[0], -t[1], -t[2]];
    if how == Build::ThroughRest {
        // the rest matrix moved by minus the rest translation, then the turn (the pose's by the
        // rest's) scaled by the state and put back by both translations
        let mut rest = node.rest;
        pre_translate(&mut rest, minus);
        let mut turn = quaternion_matrix(compose(st.rotation, node.rotation));
        scale_rows(&mut turn, st.scale);
        translate(&mut turn, back);
        return concat(&rest, &turn);
    }
    // the pose's turn (scaled if it says so) about the rest translation
    let mut m = quaternion_matrix(st.rotation);
    if how == Build::TurnedScaled {
        scale_rows(&mut m, st.scale);
    }
    pre_translate(&mut m, minus);
    translate(&mut m, back);
    m
}

/// The bone matrices: a node's own times its parent's, the first node's parent the root; a node
/// without `POSED` is its parent's as it is. Last, a controller scale other than 1 scales every
/// matrix's 3 x 3. The original then writes each matrix's translation over its state's.
pub fn bone_matrices(nodes: &[CharNode], states: &[NodeState], root: &Mat4, mode: PoseMode) -> Vec<Mat4> {
    let mut out: Vec<Mat4> = Vec::with_capacity(nodes.len());
    for (i, (node, st)) in nodes.iter().zip(states).enumerate() {
        let parent = if i == 0 { root } else { &out[node.parent as usize] };
        let how = build(node, st, mode);
        let m = if how == Build::Inherit { *parent } else { concat(&local(node, st, how), parent) };
        out.push(m);
    }
    if mode.scale != 1.0 {
        for m in &mut out {
            for row in 0..3 {
                for c in 0..3 {
                    m[row * 4 + c] *= mode.scale;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod pose_tests {
    use super::*;

    fn near(a: &Mat4, b: &Mat4) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;

    #[test]
    fn identity_quaternion_is_identity() {
        assert_eq!(quaternion_matrix([0., 0., 0., 1.]), IDENTITY);
    }

    #[test]
    fn quarter_turn_about_z() {
        let want = [0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
        assert!(near(&quaternion_matrix([0., 0., H, H]), &want));
    }

    #[test]
    fn concat_with_identity() {
        let m: Mat4 = std::array::from_fn(|i| i as f32 * 0.25 - 1.0);
        assert_eq!(concat(&m, &IDENTITY), m);
        assert_eq!(concat(&IDENTITY, &m), m);
    }

    #[test]
    fn concat_applies_a_first() {
        let mut a = IDENTITY;
        translate(&mut a, [1., 0., 0.]);
        let mut b = IDENTITY;
        translate(&mut b, [0., 2., 0.]);
        assert_eq!(&concat(&a, &b)[12..15], &[1., 2., 0.]);
    }

    #[test]
    fn pre_translate_goes_through_the_rotation() {
        // (1,0,0) turned a quarter about z is (0,1,0)
        let mut m = quaternion_matrix([0., 0., H, H]);
        let mut want = m;
        pre_translate(&mut m, [1., 0., 0.]);
        want[13] = 1.0;
        assert!(near(&m, &want));
    }

    #[test]
    fn scale_rows_scales_each_row() {
        let mut m = IDENTITY;
        scale_rows(&mut m, [2., 3., 4.]);
        assert_eq!((m[0], m[5], m[10], m[15]), (2., 3., 4., 1.));
    }

    #[test]
    fn unposed_nodes_copy_their_parent() {
        let node = CharNode { parent: -1, translation: [0.; 3], rotation: [0., 0., 0., 1.], rest: IDENTITY, direct: false };
        let st = NodeState { flags: 0, rotation: [0., 0., 0., 1.], translation: [0.; 3], scale: [1.; 3] };
        let mut root = IDENTITY;
        root[12] = 5.0;
        let out = bone_matrices(&[node], &[st], &root, PoseMode { through_rest: false, scale: 1.0 });
        assert_eq!(out[0], root);
    }

    #[test]
    fn posed_node_turns_about_its_rest_translation() {
        // a quarter turn about z of a node resting at (1,0,0) leaves that point where it is
        let node = CharNode { parent: -1, translation: [1., 0., 0.], rotation: [0., 0., 0., 1.], rest: IDENTITY, direct: true };
        let st = NodeState { flags: POSED, rotation: [0., 0., H, H], translation: [0.; 3], scale: [1.; 3] };
        let out = bone_matrices(&[node], &[st], &IDENTITY, PoseMode { through_rest: false, scale: 1.0 });
        let p = [1., 0., 0.];
        let m = &out[0];
        let moved: Vec<f32> = (0..3).map(|j| p[0] * m[j] + p[1] * m[4 + j] + p[2] * m[8 + j] + m[12 + j]).collect();
        assert!(moved.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-5), "{moved:?}");
    }

    #[test]
    fn compose_with_identity_rest_is_the_pose() {
        let q = [0.1, 0.2, 0.3, 0.9];
        for (a, b) in compose(q, [0., 0., 0., 1.]).iter().zip(&q) {
            assert!((a - b).abs() < 1e-6);
        }
    }
}
