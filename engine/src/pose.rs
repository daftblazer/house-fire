//! Posing a track's animation onto a skeleton's node states, as EA's EAnimController::Animate
//! (Bustin' Out 801581C8) does it: for each node a local pose is made from the node's channels
//! at the track's place, then folded into the node's state by the track's weight (skipped at 0,
//! copied at 1, blended otherwise, or layered on top when the track says so). Every operation is
//! in single precision, fused where the original's code fuses. `docs/assets.md`, "A node's seven
//! channels".

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

/// What a pose could not be made or folded with: not decoded yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unsupported {
    /// A scalar channel (word 5 or 6) given by a stream (801BD7BC).
    ScalarStream,
    /// A rotation blend that slerps (801B4B50's arccosine branch).
    Slerp,
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
    /// clear its flag channel is read. The streams advance only when a channel is a stream, in the game's
    /// order (rotation, word 2, word 1, flag, word 4).
    pub fn local(&mut self, anim: &Anim, i: usize, place: f32, state_flags: u32) -> Result<Pose, Unsupported> {
        let node = &anim.nodes[i];
        let words = &anim.bit_words;
        let streams = &mut self.nodes[i];
        let mut pose = Pose { flags: if state_flags & FLAGS_KEEP != 0 { state_flags & FLAGS_KEPT } else { 0 }, ..Pose::default() };
        let constants = |at: usize, n: usize| anim.constants.get(at..at + n).map(|c| c.to_vec()).unwrap_or_else(|| vec![0.0; n]);

        // word 0: the rotation, or the identity
        match Channel::of(node[0]) {
            Channel::None => {}
            ch => {
                pose.flags |= 1;
                pose.rotation = match (ch, streams.rotation.as_mut()) {
                    (Channel::Constant(at), _) => constants(at, 4).try_into().unwrap(),
                    (_, Some(s)) => s.read(words, place),
                    _ => unreachable!(),
                };
            }
        }
        // the vector channels: (word, slot in the pose, flag bits set, default)
        let mut vec3 = |w: usize, k: usize, flags: u32, out: &mut [f32; 3]| {
            let ch = Channel::of(node[w]);
            if ch == Channel::None {
                return;
            }
            pose.flags |= flags;
            *out = match (ch, streams.vectors[k].as_mut()) {
                (Channel::Constant(at), _) => constants(at, 3).try_into().unwrap(),
                (_, Some(s)) => s.read(words, place),
                _ => unreachable!(),
            };
        };
        let mut translation = pose.translation;
        vec3(2, 1, 1, &mut translation);
        let mut scale = pose.scale;
        vec3(1, 0, 3, &mut scale);
        pose.translation = translation;
        pose.scale = scale;
        if scale[0] != scale[1] || scale[0] != scale[2] {
            pose.flags |= FLAG_UNEVEN_SCALE;
        }
        // word 3: the flag channel, read only while the state's flag bit 0x10 is clear (8015852C); the
        // pose's bit 8 is the bit's opposite (80158540)
        if let Some(f) = FlagStream::of(node[3]) {
            if state_flags & FLAGS_KEEP == 0 {
                pose.flags = if f.read(words, anim.bits, place) { pose.flags & !FLAG_CHANNEL } else { pose.flags | FLAG_CHANNEL };
            }
        }
        let mut vector = pose.vector;
        let mut flags = pose.flags;
        {
            let ch = Channel::of(node[4]);
            if ch != Channel::None {
                flags |= 0x20;
                vector = match (ch, streams.vectors[2].as_mut()) {
                    (Channel::Constant(at), _) => constants(at, 3).try_into().unwrap(),
                    (_, Some(s)) => s.read(words, place),
                    _ => unreachable!(),
                };
            }
        }
        pose.vector = vector;
        pose.flags = flags;
        // words 5 and 6: scalars, a constant or a stream not decoded here
        let mut unsupported = None;
        for (w, bit) in [(5, 0x40), (6, 0x80)] {
            match Channel::of(node[w]) {
                Channel::None => {}
                Channel::Constant(at) => {
                    pose.flags |= bit;
                    let v = anim.constants.get(at).copied().unwrap_or(0.0);
                    if w == 5 { pose.scalar_a = v } else { pose.scalar_b = v }
                }
                Channel::Stream(_) => {
                    pose.flags |= bit;
                    unsupported = Some(Unsupported::ScalarStream);
                }
            }
        }
        match unsupported {
            Some(u) => Err(u),
            None => Ok(pose),
        }
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
pub fn fold(state: &Pose, pose: &Pose, fold: Fold) -> Result<Pose, Unsupported> {
    Ok(match fold {
        Fold::Skip => *state,
        Fold::Copy => *pose,
        Fold::Blend(w) => blend(state, pose, w)?,
        Fold::Layer(w) => layer(state, pose, w)?,
    })
}

/// a + (b - a) t per component (801587D4: the difference and its product each rounded).
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Blend (801587D4): the flags or'd, the rotation blended (`rotation_blend`), the translation
/// and the scale moved toward the pose by the weight; the rest stays the state's.
pub fn blend(state: &Pose, pose: &Pose, w: f32) -> Result<Pose, Unsupported> {
    Ok(Pose {
        flags: state.flags | pose.flags,
        rotation: blend_rotation(state.rotation, pose.rotation, w)?,
        translation: lerp3(state.translation, pose.translation, w),
        scale: lerp3(state.scale, pose.scale, w),
        ..*state
    })
}

/// Layer (80158964): the flags or'd, the rotation the pose's rotation raised to the weight
/// (801B5154) multiplied onto the state's, the translation added by the weight, the scale
/// scaled by the pose's by the weight; the rest stays the state's.
pub fn layer(state: &Pose, pose: &Pose, w: f32) -> Result<Pose, Unsupported> {
    let t = rotation_power(pose.rotation, w)?;
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
    Ok(Pose {
        flags: state.flags | pose.flags,
        rotation: [x, ry, rz, rw],
        translation: core::array::from_fn(|i| state.translation[i] + w * pose.translation[i]),
        scale: core::array::from_fn(|i| {
            let (s, l) = (state.scale[i], pose.scale[i]);
            s + (s * l - s) * w
        }),
        ..*state
    })
}

/// The rotation `q` raised to `t` (801B5154): the identity at 0, `q` at 1, else the blend of the
/// identity toward `q`.
fn rotation_power(q: [f32; 4], t: f32) -> Result<[f32; 4], Unsupported> {
    const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    if t == 0.0 {
        Ok(IDENTITY)
    } else if t == 1.0 {
        Ok(q)
    } else {
        blend_rotation(IDENTITY, q, t)
    }
}

/// The dot at which a blend takes the straight line (802C3578).
const NLERP_DOT: f32 = f32::from_bits(0x3f7f_be77);

/// Two rotations blended by `t` (801B4B50): `b` flipped to the shorter way round, then a at 0,
/// b at 1, the normalized straight line when the two are close (dot at least `NLERP_DOT`),
/// else the spherical one, which needs the arccosine (not decoded: `Unsupported::Slerp`).
pub fn blend_rotation(a: [f32; 4], mut b: [f32; 4], t: f32) -> Result<[f32; 4], Unsupported> {
    let fma = |x: f32, y: f32, z: f32| (x as f64).mul_add(y as f64, z as f64) as f32;
    let mut dot = fma(a[3], b[3], fma(a[2], b[2], fma(a[0], b[0], a[1] * b[1])));
    if dot < 0.0 {
        b = b.map(|v| -v);
        dot = -dot;
    }
    if t == 0.0 {
        return Ok(a);
    }
    if t == 1.0 {
        return Ok(b);
    }
    if dot >= NLERP_DOT {
        let sum: [f32; 4] = core::array::from_fn(|i| a[i] + t * (b[i] - a[i]));
        return Ok(anim::unit_or_identity(sum));
    }
    Err(Unsupported::Slerp)
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
        assert_eq!(fold(&a, &b, Fold::Skip).unwrap(), a);
        assert_eq!(fold(&a, &b, Fold::Copy).unwrap(), b);
    }

    #[test]
    fn blend_moves_the_vectors_and_keeps_the_rest() {
        let a = Pose { flags: 1, vector: [7.0; 3], ..pose([0.0, 0.0, 0.0, 1.0], [0.0; 3], [1.0; 3]) };
        let b = Pose { flags: 2, vector: [9.0; 3], ..pose([0.0, 0.0, 0.0, 1.0], [2.0, 4.0, 8.0], [3.0; 3]) };
        let r = fold(&a, &b, Fold::Blend(0.5)).unwrap();
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
        assert_eq!(blend_rotation(a, b, 0.5).unwrap(), a);
        assert_eq!(blend_rotation(a, b, 1.0).unwrap(), a);
        assert_eq!(blend_rotation(a, [1.0, 0.0, 0.0, 0.0], 0.5), Err(Unsupported::Slerp));
    }

    #[test]
    fn layer_adds_the_translation_and_scales() {
        let a = pose([0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 3.0], [2.0; 3]);
        let b = pose([0.0, 0.0, 0.0, 1.0], [4.0, 4.0, 4.0], [3.0; 3]);
        let r = layer(&a, &b, 0.5).unwrap();
        assert_eq!(r.translation, [3.0, 4.0, 5.0]);
        // 2 + (2 * 3 - 2) * 0.5
        assert_eq!(r.scale, [4.0; 3]);
        assert_eq!(r.rotation, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn the_root_fix_replaces_the_named_components() {
        let mut p = Pose::default();
        fix_root(&mut p, 0b101, [1.0, 2.0, 3.0]);
        assert_eq!(p.translation, [1.0, 0.0, 3.0]);
    }
}
