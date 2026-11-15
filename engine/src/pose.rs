//! Posing a track's animation onto a skeleton's node states, as EA's EAnimController::Animate
//! (Bustin' Out 801581C8) does it: for each node a local pose is made from the node's channels
//! at the track's place, then folded into the node's state by the track's weight (skipped at 0,
//! copied at 1, blended otherwise, or layered on top when the track says so). Every operation is
//! in single precision, fused where the original's code fuses. `docs/assets.md`, "A node's seven
//! channels".

use crate::mathf;
use crate::anim::{self, Anim, Channel, FlagStream, RotStream, Stream};

/// The node state flag whose set bit carries the state's 0x18 bits into the pose and stops the flag channel being read.
pub const FLAGS_KEEP: u32 = 0x10;
/// The flag bits a node state hands on to its next local pose (801582F8: the state's flags and 0x18).
const FLAGS_KEPT: u32 = 0x18;
/// The local pose flag the flag channel sets (a clear bit) or clears (a set bit).
pub const FLAG_CHANNEL: u32 = 0x08;
/// 0x1: the rotation (and the translation) was given, 0x2: the scale, 0x4: the scale is not
/// uniform, 0x20, 0x40 and 0x80: the fifth to seventh channels.
pub const FLAG_UNEVEN_SCALE: u32 = 0x04;

/// A node's pose in its own terms: what both a node's local pose and its state hold (64 bytes in
/// the game, here named). The channel names follow the node's words in `anim::Node`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub flags: u32,
    /// The rotation as a quaternion (x, y, z, w); word 0.
    pub rotation: [f32; 4],
    /// The translation, word 2 (default 0).
    pub translation: [f32; 3],
    /// The scale, word 1 (default 1).
    pub scale: [f32; 3],
    /// The vector of word 4 (default 1); never blended, only copied.
    pub vector: [f32; 3],
    /// Word 5 (default 1).
    pub scalar_a: f32,
    /// Word 6 (default 10: 802B7F40).
    pub scalar_b: f32,
}

/// The default of word 6 when it has no channel (the float at 802B7F40).
pub const SCALAR_B_DEFAULT: f32 = 10.0;

impl Default for Pose {
    /// The pose of a node with no channels.
    fn default() -> Pose {
        Pose { flags: 0, rotation: [0.0, 0.0, 0.0, 1.0], translation: [0.0; 3], scale: [1.0; 3], vector: [1.0; 3], scalar_a: 1.0, scalar_b: SCALAR_B_DEFAULT }
    }
}

impl Pose {
    /// The pose in the logged 16 words (flags, rotation, translation, scale, vector, scalars).
    pub fn from_words(w: &[u32; 16]) -> Pose {
        let f = |i: usize| f32::from_bits(w[i]);
        Pose { flags: w[0], rotation: [f(1), f(2), f(3), f(4)], translation: [f(5), f(6), f(7)], scale: [f(8), f(9), f(10)], vector: [f(11), f(12), f(13)], scalar_a: f(14), scalar_b: f(15) }
    }

    /// The pose as 16 words, bit for bit.
    pub fn to_words(&self) -> [u32; 16] {
        let [x, y, z, w] = self.rotation;
        let b = f32::to_bits;
        [
            self.flags,
            b(x),
            b(y),
            b(z),
            b(w),
            b(self.translation[0]),
            b(self.translation[1]),
            b(self.translation[2]),
            b(self.scale[0]),
            b(self.scale[1]),
            b(self.scale[2]),
            b(self.vector[0]),
            b(self.vector[1]),
            b(self.vector[2]),
            b(self.scalar_a),
            b(self.scalar_b),
        ]
    }
}

/// What a local pose could not be made from: not decoded yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unsupported {
    /// A scalar channel (word 5 or 6) given by a stream (801BD7BC).
    ScalarStream,
}

/// The readers of one track's streams, a node's channels each with the window it keeps from read
/// to read (the game keeps them in the track's +148 array). Made for a track playing `anim`.
#[derive(Clone, Debug)]
pub struct TrackStreams {
    nodes: Vec<NodeStreams>,
}

#[derive(Clone, Debug, Default)]
struct NodeStreams {
    rotation: Option<RotStream>,
    /// Words 1, 2 and 4.
    vectors: [Option<Stream>; 3],
}

/// The words of the three vector channels, in `NodeStreams::vectors`' order.
const VECTOR_WORDS: [usize; 3] = [1, 2, 4];

impl TrackStreams {
    /// The streams a track starts with on `anim` (each at its first window).
    pub fn new(anim: &Anim) -> TrackStreams {
        let words = &anim.bit_words;
        let nodes = anim
            .nodes
            .iter()
            .map(|n| NodeStreams {
                rotation: match Channel::of(n[0]) {
                    Channel::Stream(at) => Some(RotStream::new(words, anim::rotation_header(words, at))),
                    _ => None,
                },
                vectors: VECTOR_WORDS.map(|w| match Channel::of(n[w]) {
                    Channel::Stream(at) => Some(Stream::new(words, anim::stream_header(words, at))),
                    _ => None,
                }),
            })
            .collect();
        TrackStreams { nodes }
    }

    /// Node `i`'s local pose at `place` (801582F4 to 8015869C). `state_flags` is the node
    /// state's flags before the pose: with `FLAGS_KEEP` set its 0x18 bits are carried, with it
    /// clear its flag channel is read. Each stream keeps its own window, so the order the game
    /// reads them in does not matter.
    pub fn local(&mut self, anim: &Anim, i: usize, place: f32, state_flags: u32) -> Result<Pose, Unsupported> {
        let node = &anim.nodes[i];
        let words = &anim.bit_words;
        let streams = &mut self.nodes[i];
        let constants = |at: usize, n: usize| -> Vec<f32> { anim.constants.get(at..at + n).map_or_else(|| vec![0.0; n], <[f32]>::to_vec) };
        let mut pose = Pose { flags: if state_flags & FLAGS_KEEP != 0 { state_flags & FLAGS_KEPT } else { 0 }, ..Pose::default() };

        // word 0: the rotation, or the identity (flag 0x1)
        match (Channel::of(node[0]), streams.rotation.as_mut()) {
            (Channel::None, _) => {}
            (Channel::Constant(at), _) => {
                pose.flags |= 1;
                pose.rotation = constants(at, 4).try_into().unwrap();
            }
            (Channel::Stream(_), stream) => {
                pose.flags |= 1;
                pose.rotation = stream.map_or(pose.rotation, |s| s.read(words, place));
            }
        }
        // the vector channels: words 2 (the translation, flags 0x1), 1 (the scale, 0x3), 4 (0x20)
        let mut vec3 = |word: usize, slot: usize, flags: u32, default: [f32; 3]| -> ([f32; 3], u32) {
            match (Channel::of(node[word]), streams.vectors[slot].as_mut()) {
                (Channel::None, _) => (default, 0),
                (Channel::Constant(at), _) => (constants(at, 3).try_into().unwrap(), flags),
                (Channel::Stream(_), stream) => (stream.map_or(default, |s| s.read(words, place)), flags),
            }
        };
        for (word, slot, flags) in [(2, 1, 0x1), (1, 0, 0x3), (4, 2, 0x20)] {
            let target = match word {
                2 => &mut pose.translation,
                1 => &mut pose.scale,
                _ => &mut pose.vector,
            };
            let (value, set) = vec3(word, slot, flags, *target);
            *target = value;
            pose.flags |= set;
        }
        if pose.scale[0] != pose.scale[1] || pose.scale[0] != pose.scale[2] {
            pose.flags |= FLAG_UNEVEN_SCALE;
        }
        // word 3: the flag channel, read only while the state's flag bit 0x10 is clear (8015852C);
        // the pose's bit 8 is the bit's opposite (80158540)
        if let Some(f) = FlagStream::of(node[3])
            && state_flags & FLAGS_KEEP == 0
        {
            pose.flags = if f.read(words, anim.bits, place) { pose.flags & !FLAG_CHANNEL } else { pose.flags | FLAG_CHANNEL };
        }
        // words 5 and 6: scalars (flags 0x40 and 0x80), a constant or a stream not decoded here
        let mut unsupported = None;
        for (word, flag) in [(5, 0x40), (6, 0x80)] {
            match Channel::of(node[word]) {
                Channel::None => {}
                Channel::Constant(at) => {
                    pose.flags |= flag;
                    let v = anim.constants.get(at).copied().unwrap_or(0.0);
                    if word == 5 { pose.scalar_a = v } else { pose.scalar_b = v }
                }
                Channel::Stream(_) => {
                    pose.flags |= flag;
                    unsupported = Some(Unsupported::ScalarStream);
                }
            }
        }
        unsupported.map_or(Ok(pose), Err)
    }
}

/// The root node's translation fix (FixAccumulationForRootNode 80158184): each of the three
/// components the track's flags (bits 0 to 2) name is replaced by the track's own value.
pub fn fix_root(pose: &mut Pose, track_flags: u32, values: [f32; 3]) {
    for (i, v) in values.into_iter().enumerate() {
        if track_flags >> i & 1 != 0 {
            pose.translation[i] = v;
        }
    }
}

/// How a pose is folded into a node state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fold {
    /// Weight 0: nothing.
    Skip,
    /// Weight 1: the state becomes the pose.
    Copy,
    /// The pose is blended in by the weight (Blend 801587D4).
    Blend(f32),
    /// The pose is layered on top by the weight (Layer 80158964; the track's layer flag, +8 bit 8).
    Layer(f32),
}

impl Fold {
    /// The fold Animate (801586B4) picks: the weight 0 skips, then the layer flag layers, else
    /// weight 1 copies and the rest blends.
    pub fn of(weight: f32, layer: bool) -> Fold {
        if weight == 0.0 {
            Fold::Skip
        } else if layer {
            Fold::Layer(weight)
        } else if weight == 1.0 {
            Fold::Copy
        } else {
            Fold::Blend(weight)
        }
    }
}

/// `state` with `pose` folded in as `fold` says.
pub fn fold(state: &Pose, pose: &Pose, fold: Fold) -> Pose {
    match fold {
        Fold::Skip => *state,
        Fold::Copy => *pose,
        Fold::Blend(w) => blend(state, pose, w),
        Fold::Layer(w) => layer(state, pose, w),
    }
}

/// a + (b - a) t per component (801587D4: the difference and its product each rounded).
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Blend (801587D4): the flags or'd, the rotation blended (`rotation_blend`), the translation
/// and the scale moved toward the pose by the weight; the rest stays the state's.
pub fn blend(state: &Pose, pose: &Pose, w: f32) -> Pose {
    Pose {
        flags: state.flags | pose.flags,
        rotation: blend_rotation(state.rotation, pose.rotation, w),
        translation: lerp3(state.translation, pose.translation, w),
        scale: lerp3(state.scale, pose.scale, w),
        ..*state
    }
}

/// Layer (80158964): the flags or'd, the rotation the pose's rotation raised to the weight
/// (801B5154) multiplied onto the state's, the translation added by the weight, the scale
/// scaled by the pose's by the weight; the rest stays the state's.
pub fn layer(state: &Pose, pose: &Pose, w: f32) -> Pose {
    let t = rotation_power(pose.rotation, w);
    let s = state.rotation;
    // the product t * s, the terms fused as the code fuses them (80158A00 to 80158A34)
    let fma = |a: f32, b: f32, c: f32| (a as f64).mul_add(b as f64, c as f64) as f32;
    let [tx, ty, tz, tw] = t;
    let [sx, sy, sz, sw] = s;
    let a = fma(tw, sx, -(tz * sy));
    let b = fma(-tx, sx, -(ty * sy));
    let c = fma(-ty, sx, tx * sy);
    let a = fma(ty, sz, a);
    let d = fma(tz, sx, tw * sy);
    let x = fma(tx, sw, a);
    let d = d - tx * sz;
    let e = fma(tw, sz, c);
    let b = b - tz * sz;
    let rw = fma(tw, sw, b);
    let ry = fma(ty, sw, d);
    let rz = fma(tz, sw, e);
    Pose {
        flags: state.flags | pose.flags,
        rotation: [x, ry, rz, rw],
        translation: core::array::from_fn(|i| state.translation[i] + w * pose.translation[i]),
        scale: core::array::from_fn(|i| {
            let (s, l) = (state.scale[i], pose.scale[i]);
            s + (s * l - s) * w
        }),
        ..*state
    }
}

/// The rotation `q` raised to `t` (801B5154): the identity at 0, `q` at 1, else the blend of the
/// identity toward `q`.
fn rotation_power(q: [f32; 4], t: f32) -> [f32; 4] {
    const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    if t == 0.0 {
        IDENTITY
    } else if t == 1.0 {
        q
    } else {
        blend_rotation(IDENTITY, q, t)
    }
}

/// The dot at which a blend takes the straight line (802C3578).
const NLERP_DOT: f32 = f32::from_bits(0x3f7f_be77);

/// Two rotations blended by `t` (801B4B50): `b` flipped to the shorter way round, then a at 0,
/// b at 1, the normalized straight line when the two are close (dot at least `NLERP_DOT`),
/// else the spherical interpolation, sin((1 - t) w) / sin w a + sin(t w) / sin w b with
/// w = acos(dot).
pub fn blend_rotation(a: [f32; 4], mut b: [f32; 4], t: f32) -> [f32; 4] {
    let mut dot = fma(a[3], b[3], fma(a[2], b[2], fma(a[0], b[0], a[1] * b[1])));
    if dot < 0.0 {
        b = b.map(|v| -v);
        dot = -dot;
    }
    if t == 0.0 {
        return a;
    }
    if t == 1.0 {
        return b;
    }
    if dot >= NLERP_DOT {
        let sum: [f32; 4] = core::array::from_fn(|i| a[i] + t * (b[i] - a[i]));
        return anim::unit_or_identity(sum);
    }
    let sin = |x: f32| mathf::sin(x).unwrap_or(0.0);
    let omega = acos(dot);
    let inverse = 1.0 / sin(omega);
    let wa = sin((1.0 - t) * omega) * inverse;
    let wb = sin(t * omega) * inverse;
    core::array::from_fn(|i| wa * a[i] + wb * b[i])
}

/// a x b + c, fused, rounded to single.
fn fma(a: f32, b: f32, c: f32) -> f32 {
    (a as f64).mul_add(b as f64, c as f64) as f32
}

/// The game's arccosine (8010DB14): fdlibm's e_acos in single precision, the fused steps fused.
/// The constants are the game's small-data floats (r13 - 30784 to r13 - 30716, r13 = 803833E0).
pub fn acos(x: f32) -> f32 {
    const PI: f32 = f32::from_bits(0x4049_0fdb);
    const PIO2: f32 = f32::from_bits(0x3fc9_0fdb);
    const PIO2_HI: f32 = f32::from_bits(0x3fc9_0fda);
    const PIO2_LO: f32 = f32::from_bits(0x33a2_2168);
    const PI_HI: f32 = f32::from_bits(0x4049_0fda);
    let pc: [f32; 6] = [0x3e2a_aaab, 0xbea6_b090, 0x3e4e_0aa8, 0xbd24_1146, 0x3a4f_7f04, 0x3811_ef08].map(f32::from_bits);
    let qc: [f32; 4] = [0xc019_d139, 0x4001_572d, 0xbf30_3361, 0x3d9d_c62e].map(f32::from_bits);
    // p(z) / q(z) as fdlibm has them: z (pS0 + z (pS1 + ...)) and 1 + z (qS1 + ...)
    let ratio = |z: f32| {
        let p = fma(z, fma(z, fma(z, fma(z, fma(z, pc[5], pc[4]), pc[3]), pc[2]), pc[1]), pc[0]);
        let q = fma(z, fma(z, fma(z, fma(z, qc[3], qc[2]), qc[1]), qc[0]), 1.0);
        (z * p) / q
    };
    let ix = x.to_bits() & 0x7fff_ffff;
    if ix == 0x3f80_0000 {
        return if x > 0.0 { 0.0 } else { PI };
    }
    if ix > 0x3f80_0000 {
        return f32::NAN;
    }
    if ix < 0x3f00_0000 {
        if ix <= 0x2300_0000 {
            return PIO2;
        }
        let r = ratio(x * x);
        return PIO2_HI - (x - (PIO2_LO - x * r));
    }
    if x < 0.0 {
        let z = (1.0 + x) * 0.5;
        let s = z.sqrt();
        let w = fma(ratio(z), s, -PIO2_LO);
        return PI_HI - 2.0 * (s + w);
    }
    let z = (1.0 - x) * 0.5;
    let s = z.sqrt();
    // df: s without its low 12 bits, so that df * df is exact
    let df = f32::from_bits(s.to_bits() & 0xffff_f000);
    let c = (z - df * df) / (s + df);
    let w = fma(ratio(z), s, c);
    2.0 * (df + w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(rotation: [f32; 4], translation: [f32; 3], scale: [f32; 3]) -> Pose {
        Pose { rotation, translation, scale, ..Pose::default() }
    }

    #[test]
    fn words_round_trip() {
        let p = Pose { flags: 0x1b, scalar_a: -0.0, ..pose([0.1, 0.2, 0.3, 0.4], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]) };
        assert_eq!(Pose::from_words(&p.to_words()), p);
        assert_eq!(p.to_words()[14], (-0.0f32).to_bits());
    }

    #[test]
    fn the_fold_picks_by_weight() {
        assert_eq!(Fold::of(0.0, false), Fold::Skip);
        assert_eq!(Fold::of(0.0, true), Fold::Skip);
        assert_eq!(Fold::of(1.0, false), Fold::Copy);
        assert_eq!(Fold::of(1.0, true), Fold::Layer(1.0));
        assert_eq!(Fold::of(0.25, false), Fold::Blend(0.25));
    }

    #[test]
    fn skip_and_copy() {
        let a = pose([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0], [1.0; 3]);
        let b = pose([0.0, 1.0, 0.0, 0.0], [2.0, 3.0, 4.0], [2.0; 3]);
        assert_eq!(fold(&a, &b, Fold::Skip), a);
        assert_eq!(fold(&a, &b, Fold::Copy), b);
    }

    #[test]
    fn blend_moves_the_vectors_and_keeps_the_rest() {
        let a = Pose { flags: 1, vector: [7.0; 3], ..pose([0.0, 0.0, 0.0, 1.0], [0.0; 3], [1.0; 3]) };
        let b = Pose { flags: 2, vector: [9.0; 3], ..pose([0.0, 0.0, 0.0, 1.0], [2.0, 4.0, 8.0], [3.0; 3]) };
        let r = fold(&a, &b, Fold::Blend(0.5));
        assert_eq!(r.translation, [1.0, 2.0, 4.0]);
        assert_eq!(r.scale, [2.0; 3]);
        assert_eq!(r.flags, 3);
        assert_eq!(r.vector, [7.0; 3]);
        assert_eq!(r.rotation, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn blend_takes_the_shorter_way() {
        let a = [0.0, 0.0, 0.0, 1.0];
        let b = [0.0, 0.0, 0.0, -1.0];
        assert_eq!(blend_rotation(a, b, 0.5), a);
        assert_eq!(blend_rotation(a, b, 1.0), a);
    }

    #[test]
    fn layer_adds_the_translation_and_scales() {
        let a = pose([0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 3.0], [2.0; 3]);
        let b = pose([0.0, 0.0, 0.0, 1.0], [4.0, 4.0, 4.0], [3.0; 3]);
        let r = layer(&a, &b, 0.5);
        assert_eq!(r.translation, [3.0, 4.0, 5.0]);
        // 2 + (2 * 3 - 2) * 0.5
        assert_eq!(r.scale, [4.0; 3]);
        assert_eq!(r.rotation, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn the_arccosine_is_close_everywhere() {
        for k in -100..=100 {
            let x = k as f32 / 100.0;
            assert!((acos(x) - x.acos()).abs() < 2e-6, "acos({x})");
        }
        assert_eq!(acos(1.0), 0.0);
        assert!((acos(-1.0) - core::f32::consts::PI).abs() < 1e-6);
        assert!(acos(1.5).is_nan());
    }

    #[test]
    fn a_far_blend_is_spherical() {
        // half way from the identity to a quarter turn about z: an eighth of a turn
        let quarter = core::f32::consts::FRAC_PI_4;
        let q = [0.0, 0.0, quarter.sin(), quarter.cos()];
        let half = blend_rotation([0.0, 0.0, 0.0, 1.0], q, 0.5);
        let eighth = core::f32::consts::FRAC_PI_8;
        assert!((half[2] - eighth.sin()).abs() < 1e-5 && (half[3] - eighth.cos()).abs() < 1e-5, "{half:?}");
    }

    #[test]
    fn a_layer_of_a_whole_rotation_multiplies() {
        let quarter = core::f32::consts::FRAC_PI_4;
        let q = [0.0, 0.0, quarter.sin(), quarter.cos()];
        let a = pose(q, [0.0; 3], [1.0; 3]);
        let r = layer(&a, &pose(q, [0.0; 3], [1.0; 3]), 1.0);
        // two quarter-turn halves: a half turn about z
        assert!((r.rotation[2] - 1.0).abs() < 1e-6 && r.rotation[3].abs() < 1e-6, "{:?}", r.rotation);
    }

    #[test]
    fn the_root_fix_replaces_the_named_components() {
        let mut p = Pose::default();
        fix_root(&mut p, 0b101, [1.0, 2.0, 3.0]);
        assert_eq!(p.translation, [1.0, 0.0, 3.0]);
    }
}
