//! A frame of the lot from the observer log's records: which model each draw is, its world
//! matrix, material and light set, as the original's device had them (`docs/assets.md`, "The
//! renderer"). The records are a stream of draws with the state set before or during each
//! one; this walks them in order, carrying the state the way the device does.

use crate::frame::*;
use housefire::arc;
use housefire::disc::Disc;
use housefire::draw::{Part, PartIndex};
use housefire::model::SubModel;
use housefire::texture::{self, Kind};
use housefire::tree_log::{LightsRec, Log, MaterialRec, SkinVerticesRec, StateRec, StripRec, StripVertex, TexLoadRec, TlutLoadRec};
use std::collections::{BTreeMap, HashMap};

/// The alpha (of 255) below which a textured opaque pixel is not drawn, for a log without the
/// state records (';'): R1's stand-in, 0.5. The records give 127 for most such draws.
pub const ALPHA_REF: u8 = 128;

/// A layer's flag (the low byte of its flag word, 0x08 for most materials) that goes with the
/// translucent ones (windows, the aquarium, the bottle lamp, water): drawn blended by source
/// alpha, with no alpha test. Read from the log's material records, and checked by the pixels
/// those draws cover (`docs/assets.md`, "The renderer"); the blend state itself is not logged.
pub const LAYER_BLENDED: u8 = 0x40;

/// Whether a state's viewport (`StateRec::viewport`: left, top, width, height, near, far) is
/// an inner one: the lot's is the whole 640 by 448 picture.
fn inner_viewport(v: &[f32; 6]) -> Option<Viewport> {
    (v[2] < 640.0 || v[3] < 448.0).then_some(Viewport { x: v[0], y: v[1], width: v[2], height: v[3] })
}

/// The portrait's backdrop: the rectangle behind the head, filled before its draws (the 2D layer's
/// quads at the same place use viewports shifted half a pixel, `StateRec::viewport`; the quads'
/// vertices are in two display lists the log does not hold). Its pixels are one colour in the reference (`PORTRAIT_BACKDROP_COLOUR`, 54 pixels wide from x 50 and 56
/// tall from y 347, read off `world_02400.png`: the dark shoulders cover its bottom rows). It is
/// not one of the strips the log records (the two display-list calls ahead of the portrait's
/// draws, 'g' records G196 and G197, are the 2D layer's and not decoded); fitted for these frames.
pub const PORTRAIT_BACKDROP: Viewport = Viewport { x: 50.0, y: 347.0, width: 54.0, height: 56.0 };
/// The backdrop's colour in 8-bit RGB, the reference's own (218, 238, 254).
pub const PORTRAIT_BACKDROP_COLOUR: [u8; 3] = [218, 238, 254];

/// What a draw of the frame is, for the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// A strip of short arrays (opcode 56) of a model part on one bone.
    RigidStrip,
    /// A model drawn from its display list (opcode 66).
    DisplayList,
    /// A part blended over several bones (a Sim's skin).
    Skinned,
    /// Drawn with lighting off (the projected shadows, in black).
    Shadow,
    /// Drawn through another projection than the lot's camera (the interface's).
    Overlay,
    /// A floor's strip of tiles ('g' records, opcode 42).
    Floor,
    /// A floor strip drawn again for the shadows' mask: alpha only.
    FloorMask,
    /// Another command (not a draw the lot's frame shows).
    Other,
    /// A strip whose vertices are not in `models.arc` (made at run time) or whose model is not read.
    Unidentified,
}

impl Class {
    pub const ALL: [Class; 9] = [Class::RigidStrip, Class::DisplayList, Class::Skinned, Class::Shadow, Class::Floor, Class::FloorMask, Class::Overlay, Class::Other, Class::Unidentified];

    pub fn name(self) -> &'static str {
        match self {
            Class::RigidStrip => "rigid model strip",
            Class::DisplayList => "display-list model draw",
            Class::Skinned => "skinned (the Sims' parts)",
            Class::Shadow => "lighting off, shadows",
            Class::Floor => "floors' strips",
            Class::FloorMask => "floors again, the shadows' mask",
            Class::Overlay => "other projection, interface (the portrait)",
            Class::Other => "other command (not drawn)",
            Class::Unidentified => "unidentified",
        }
    }

    /// Whether the renderer draws it.
    pub fn drawn(self) -> bool {
        matches!(self, Class::RigidStrip | Class::DisplayList | Class::Skinned | Class::Shadow | Class::Floor | Class::FloorMask | Class::Overlay)
    }
}

/// One draw of the log's frame: its class, its place among the strips, and what it lacked.
#[derive(Clone, Debug)]
pub struct DrawInfo {
    pub class: Class,
    pub strip: usize,
    pub model: String,
    /// Whether its texture was not found among the game's textures (drawn white).
    pub missing_texture: bool,
    /// The names of the textures bound to its layers.
    pub textures: [Option<String>; 2],
    /// The layers' flag words (bytes 4 to 8 of each).
    pub flags: [u32; 2],
    /// Whether the material is of a kind not drawn yet (a sphere map, the added-by-table blend).
    pub unsupported: Option<&'static str>,
    /// For a skinned draw, its '$' record (`Log::skins`): the bones it was drawn with.
    pub skin: Option<usize>,
}

/// A texture of `textures.arc` or `datasets.arc`: where its bytes are.
pub(crate) struct TextureSource {
    pub(crate) name: String,
    /// The LFXT header's flags word.
    pub(crate) flags: u16,
    in_datasets: bool,
    /// The LFXT's offset in its archive's data.
    at: usize,
}

/// The game's files a frame is made from.
pub struct Assets {
    pub(crate) parts: PartIndex,
    textures_arc: Vec<u8>,
    datasets_arc: Vec<u8>,
    /// FNV-1a of a texture's first level, as the observer hashes the images it sees.
    pub(crate) by_hash: HashMap<u64, TextureSource>,
    /// A texture's first level's FNV-1a by the CRC-32 of its name in capitals (how a shader's
    /// layer names it); `textures.arc`'s first, then the datasets'.
    pub(crate) by_name: HashMap<u32, u64>,
}

fn fnv1a(b: &[u8]) -> u64 {
    b.iter().fold(1469598103934665603u64, |h, &x| (h ^ x as u64).wrapping_mul(1099511628211))
}

impl Assets {
    pub fn new(disc: &Disc) -> Assets {
        let parts = PartIndex::new(&disc.read("models.arc").unwrap_or_default());
        let textures_arc = disc.read("textures.arc").unwrap_or_default();
        let datasets_arc = disc.read("datasets.arc").unwrap_or_default();
        let mut by_hash = HashMap::new();
        let mut by_name = HashMap::new();
        for e in arc::entries(&textures_arc).unwrap_or_default() {
            let b = arc::bytes(&textures_arc, &e);
            if let Some(t) = texture::read(b) {
                let first = (t.width as usize * t.height as usize * t.kind.bits() as usize) / 8;
                let hash = fnv1a(&b[t.texels.0..t.texels.0 + first]);
                by_name.entry(e.id).or_insert(hash);
                by_name.entry(housefire::shader::name_hash(&t.name)).or_insert(hash);
                by_hash.entry(hash).or_insert(TextureSource { name: t.name.clone(), flags: t.flags, in_datasets: false, at: e.offset as usize });
            }
        }
        // those kept inside datasets.arc's entries (the lots' and objects' own)
        for e in arc::entries(&datasets_arc).unwrap_or_default() {
            let b = arc::bytes(&datasets_arc, &e);
            let mut p = 0;
            while let Some(q) = b[p..].windows(4).position(|w| w == b"LFXT") {
                let at = p + q;
                if let Some((t, end)) = texture::read_prefix(&b[at..]) {
                    let first = (t.width as usize * t.height as usize * t.kind.bits() as usize) / 8;
                    let hash = fnv1a(&b[at + t.texels.0..at + t.texels.0 + first]);
                    by_name.entry(housefire::shader::name_hash(&t.name)).or_insert(hash);
                    by_hash.entry(hash).or_insert(TextureSource { name: format!("{}/{}", e.name, t.name), flags: t.flags, in_datasets: true, at: e.offset as usize + at });
                    p = at + end;
                } else {
                    p = at + 4;
                }
            }
        }
        Assets { parts, textures_arc, datasets_arc, by_hash, by_name }
    }

    /// A texture's mip levels as RGBA (the levels whole tiles of the format hold).
    pub(crate) fn decode(&self, hash: u64) -> Option<Vec<(u32, u32, Vec<u32>)>> {
        let s = self.by_hash.get(&hash)?;
        let data = if s.in_datasets { &self.datasets_arc } else { &self.textures_arc };
        let (t, _) = texture::read_prefix(&data[s.at..])?;
        let b = &data[s.at..];
        let mut levels = Vec::new();
        let mut off = t.texels.0;
        for l in 0..t.levels {
            let (w, h) = ((t.width >> l).max(1), (t.height >> l).max(1));
            let size = (w as usize * h as usize * t.kind.bits() as usize).div_ceil(8);
            let src = b.get(off..off + size)?;
            let texels = match t.kind {
                Kind::Palette4 | Kind::Palette8 => {
                    // two IA8 palettes carry the colours: the device samples the image through
                    // each (texture maps m and m + 1) and its combiner takes red and green from
                    // the first one's intensity and alpha, blue and alpha from the second one's
                    // (`fn_8019DC7C`: stages with a yellow and a blue constant and texture swaps)
                    let (po, n) = t.palette?;
                    let first = texture::decode_indexed(t.kind, w, h, src, b.get(po..po + n * 2)?, 0)?;
                    let second = texture::decode_indexed(t.kind, w, h, src, b.get(po + n * 2..po + n * 4)?, 0)?;
                    Some(first.iter().zip(&second).map(|(&a, &b)| (a & 0xFF) | (a >> 24) << 8 | (b & 0xFF) << 16 | (b >> 24) << 24).collect())
                }
                k => texture::decode_level(k, w, h, src),
            };
            // levels smaller than the format's tile are padded in the console's layout; stop
            let Some(texels) = texels.filter(|_| w >= 8 && h >= 8 || l == 0) else { break };
            levels.push((w, h, texels));
            off += size;
        }
        (!levels.is_empty()).then_some(levels)
    }
}

/// Everything R1 draws: the frame, and each draw's account.
pub struct Scene {
    pub frame: Frame,
    /// By `Draw::id` (a draw's id is its place here plus one).
    pub drawn: Vec<DrawInfo>,
    /// Every draw of the log's frame, drawn or not.
    pub all: Vec<DrawInfo>,
    pub meshes: Vec<MeshData>,
    /// The textures wanted, by the hash the log names them: (their RGBA levels), indexed by
    /// `TextureId` once loaded in this order.
    pub textures: Vec<Vec<(u32, u32, Vec<u32>)>>,
}

fn be16(b: &[u8], p: usize) -> u16 {
    u16::from_be_bytes([b[p], b[p + 1]])
}

/// A batch of a model as a mesh: strip order as it is, or the display list's primitives.
pub(crate) fn mesh_of(sub: &SubModel, display_list: bool) -> Option<MeshData> {
    let n = sub.count;
    if sub.positions.len() != 8 * n {
        return None; // float positions (the cursors)
    }
    let vertex = |p: usize, t: Option<usize>, c: Option<usize>, nrm: usize| -> Vertex {
        let pos = &sub.positions[p * 8..p * 8 + 8];
        Vertex {
            position: [be16(pos, 0) as i16, be16(pos, 2) as i16, be16(pos, 4) as i16, 0],
            uv: t.filter(|_| !sub.texcoords.is_empty()).map_or([0, 0], |t| [be16(&sub.texcoords, t * 4) as i16, be16(&sub.texcoords, t * 4 + 2) as i16]),
            colour: match (&sub.colours, c) {
                (Some(col), Some(c)) => [col[c * 4], col[c * 4 + 1], col[c * 4 + 2], col[c * 4 + 3]],
                (Some(col), None) => [col[p * 4], col[p * 4 + 1], col[p * 4 + 2], col[p * 4 + 3]],
                _ => [255; 4],
            },
            normal: [sub.normals[nrm * 3] as i8, sub.normals[nrm * 3 + 1] as i8, sub.normals[nrm * 3 + 2] as i8, 0],
        }
    };
    let mut mesh = MeshData::default();
    if !display_list {
        mesh.vertices = (0..n).map(|i| vertex(i, Some(i), None, i)).collect();
        strip_indices(&mut mesh.indices, 0, n as u32);
        return Some(mesh);
    }
    let (_, per, list) = sub.list.as_ref()?;
    let has_t = !sub.texcoords.is_empty();
    let has_c = sub.colours.is_some();
    if *per != 2 + usize::from(has_t) + usize::from(has_c) {
        return None;
    }
    let mut p = 0;
    while p + 3 <= list.len() && list[p] != 0 {
        let (op, count) = (list[p] & 0xF8, be16(list, p + 1) as usize);
        p += 3;
        let first = mesh.vertices.len() as u32;
        for _ in 0..count {
            let idx: Vec<usize> = (0..*per).map(|k| be16(list, p + 2 * k) as usize).collect();
            p += 2 * per;
            let mut k = 1;
            let t = has_t.then(|| {
                k += 1;
                idx[k - 1]
            });
            let c = has_c.then(|| {
                k += 1;
                idx[k - 1]
            });
            if idx[0] >= n || t.is_some_and(|t| t >= n) || c.is_some_and(|c| c >= n) || idx[*per - 1] >= n {
                return None;
            }
            mesh.vertices.push(vertex(idx[0], t, c, idx[*per - 1]));
        }
        let count = count as u32;
        match op {
            0x98 => strip_indices(&mut mesh.indices, first, count),
            0x90 => mesh.indices.extend(first..first + count),
            0xA0 => (1..count.saturating_sub(1)).for_each(|i| mesh.indices.extend([first, first + i, first + i + 1])),
            0x80 => (0..count / 4).for_each(|q| {
                let b = first + q * 4;
                mesh.indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
            }),
            _ => return None,
        }
    }
    Some(mesh)
}

/// A skinned part's mesh with the vertex data the original had in memory when it drew it (a
/// '+' record: positions, weights, normals, the bone carried into the first vertex). The
/// models' own positions are the same for most parts; the Sims' faces differ in a few vertices
/// (eyes, noses and mouths shaped at run time), so the logged data is what is drawn; the texture
/// coordinates are the model's. The bone that turns a normal is the first of the largest
/// weights, the one before it when a vertex has none (the original's loop).
fn skinned_mesh(mut mesh: MeshData, plus: &SkinVerticesRec) -> Option<MeshData> {
    let n = plus.count as usize;
    if mesh.vertices.len() != n || plus.positions.len() != n * 8 || plus.weights.len() != n * 4 || plus.normals.len() != n * 4 {
        return None;
    }
    let mut heaviest = (plus.heaviest & 3) as u8;
    for (i, v) in mesh.vertices.iter_mut().enumerate() {
        let p = &plus.positions[i * 8..];
        v.position = [be16(p, 0) as i16, be16(p, 2) as i16, be16(p, 4) as i16, 0];
        let nrm = &plus.normals[i * 4..];
        v.normal = [nrm[0] as i8, nrm[1] as i8, nrm[2] as i8, 0];
        let weights: [u8; 4] = plus.weights[i * 4..i * 4 + 4].try_into().unwrap();
        let mut most = 0;
        for (j, &b) in weights.iter().enumerate() {
            if b > most {
                (most, heaviest) = (b, j as u8);
            }
        }
        mesh.skin.push(SkinWeights { weights, heaviest });
    }
    Some(mesh)
}

/// What a '+' record's vertex data is, for telling meshes of one batch apart.
fn plus_key(plus: &SkinVerticesRec) -> u64 {
    [&plus.positions, &plus.weights, &plus.normals].iter().fold(u64::from(plus.heaviest), |h, b| h ^ fnv1a(b).rotate_left(17))
}

/// A triangle strip of `count` vertices from `first`, as a list (every second triangle turned
/// so they all face the same way).
fn strip_indices(out: &mut Vec<u32>, first: u32, count: u32) {
    for i in 0..count.saturating_sub(2) {
        let a = first + i;
        if i % 2 == 0 {
            out.extend([a, a + 1, a + 2]);
        } else {
            out.extend([a + 1, a, a + 2]);
        }
    }
}

pub(crate) fn mat3x4(v: &[f32]) -> Mat3x4 {
    v[..12].try_into().unwrap()
}

pub(crate) fn apply(m: &Mat3x4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r * 4] * p[0] + m[r * 4 + 1] * p[1] + m[r * 4 + 2] * p[2] + m[r * 4 + 3])
}

/// A colour's channels in 0..1 as the game's conversion makes them bytes, back as 0..1.
pub(crate) fn byte(v: f32) -> f32 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() / 255.0
}

/// The light set the device was given (`ERC_SetLights`, the recomp's `erc_lights.cpp`): the
/// ambient and each light's colour times its tint as bytes; directionals turned by the device's
/// matrix, points moved into view space by the view the device had.
pub(crate) fn lighting_of(l: &LightsRec) -> Lighting {
    let flags = l.flags();
    if flags & 0x4000 != 0 {
        return Lighting::Black;
    }
    if flags & 0x8 == 0 {
        return Lighting::VertexColours;
    }
    let (light_tint, ambient_tint) = l.tints();
    let amb = l.ambient();
    let ambient = [0, 1, 2].map(|i| byte(amb[i] * ambient_tint[i]));
    let (nd, np) = l.counts();
    let (_, _, range_scale, k0) = l.globals();
    let turn = l.turn();
    let directionals = (0..nd as usize)
        .map(|k| {
            let d = l.directional(k);
            let colour = [0, 1, 2].map(|i| byte(light_tint[i] * d[i]));
            let v = apply(&turn, [d[3], d[4], d[5]]);
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let u = if len > 0.0 { v.map(|x| x / len) } else { v };
            // the light stands far off along the turned direction, negated by the game's constant
            Directional { colour, to_light: u.map(|x| -x) }
        })
        .collect();
    let view = l.view();
    let points = (0..np as usize)
        .map(|k| {
            let p = l.point(k);
            Point { colour: [0, 1, 2].map(|i| byte(light_tint[i] * p[4 + i])), position: apply(&view, [p[0], p[1], p[2]]), k0, k1: p[3] * range_scale }
        })
        .collect();
    Lighting::Lit { ambient, directionals, points }
}

fn wrap_of(v: u32) -> Wrap {
    match v & 3 {
        0 => Wrap::Clamp,
        1 => Wrap::Repeat,
        _ => Wrap::Mirror,
    }
}

/// A material and the textures bound to its layers, as the device's combiner stages would make
/// them (`erc_material.cpp`). The state a draw is made in beyond that (blend, depth, cull,
/// alpha compare) is guessed from the material here, and replaced by the draw's own state
/// record where the log has one (`with_state`).
pub(crate) fn material_of(m: &MaterialRec, lit: bool, bound: [Option<TextureRef>; 2]) -> (Material, Option<&'static str>) {
    let l = |i: usize, o: usize| m.layer[i][o];
    let (mode, blend, by, enabled, factor) = (l(1, 0x10), l(1, 0x11), l(1, 0x12), l(1, 0x13), l(1, 0x14));
    let sphere = m.layers == 2 && l(1, 0x16) == 2;
    let mut unsupported = None;
    let two = m.layers == 2 && bound[1].is_some() && lit && !sphere;
    let mut mat = Material { combine: Combine::Vertex, layers: [bound[0], None], blend: Blend::Opaque, cull: Cull::None, depth_test: Some(Compare::LEQUAL), depth_write: true, alpha_test: None, write_colour: true, write_alpha: true, dst_alpha: None, layer1_from_position: None, sphere: None };
    let sphere_weight = match by {
        1 => SphereWeight::PrevAlpha,
        2 => SphereWeight::Factor(f32::from(factor) / 255.0),
        _ => SphereWeight::One,
    };
    if two && mode == 0 && blend == 1 && by == 2 && enabled == 1 {
        mat.combine = Combine::Crossfade { fade: f32::from(255 - factor) / 255.0 };
        mat.layers = bound;
    } else if two && (mode == 0 || mode == 2) && enabled == 1 {
        let unlit_first = l(0, 0x15) != 0;
        match (blend, by) {
            (2, 1) => {
                mat.combine = Combine::Add { unlit_first };
                mat.blend = Blend::ADD;
            }
            (2, 2) => unsupported = Some("added by the game's table"),
            (1, 1) => {
                mat.combine = Combine::Over { by_texture_alpha: false, unlit_first };
                mat.blend = Blend::ALPHA;
            }
            (1, 0) => {
                mat.combine = Combine::Over { by_texture_alpha: true, unlit_first };
                mat.blend = Blend::ALPHA;
            }
            _ => unsupported = Some("second layer of another blend"),
        }
        mat.layers = bound;
    }
    if matches!(mat.combine, Combine::Vertex) {
        mat.layers = [bound[0], None];
        if bound[0].is_some() {
            mat.combine = Combine::Modulate { unlit: l(0, 0x15) == 1 };
            if m.layer[0][7] & LAYER_BLENDED != 0 {
                mat.blend = Blend::ALPHA;
            } else {
                mat.alpha_test = Some(AlphaTest::at_least(ALPHA_REF));
            }
            if sphere && lit && bound[1].is_some() {
                mat.sphere = Some(sphere_weight);
                mat.layers = bound;
            }
        }
    }
    (mat, unsupported)
}

/// The material in the GX state the device applied for the draw (the log's ';' record).
pub(crate) fn with_state(mut mat: Material, st: &StateRec) -> Material {
    mat.blend = if st.blend_flags & 0x40 == 0 {
        Blend::Opaque
    } else {
        match st.blend[0] {
            1 => Blend::Mix { src: Factor::from_gx(st.blend[1]), dst: Factor::from_gx(st.blend[2]) },
            3 => Blend::Subtract,
            // logic ops (2) are not met in the frames checked
            _ => Blend::Opaque,
        }
    };
    let (on, func, update) = st.z;
    mat.depth_test = on.then_some(Compare(func as u8 & 7));
    mat.depth_write = on && update;
    let (c0, r0, op, c1, r1) = st.alpha;
    let test = AlphaTest { comp: [Compare(c0 as u8 & 7), Compare(c1 as u8 & 7)], refs: [r0, r1], op: op as u8 & 3 };
    mat.alpha_test = (!test.passes_all()).then_some(test);
    mat.cull = match st.cull {
        0 => Cull::None,
        1 => Cull::Front,
        2 => Cull::Back,
        _ => Cull::All,
    };
    // the pixel engine's registers (their top byte is the register's number): colour update
    // bit 3, alpha update bit 4 of cmode0; bit 8 of cmode1 a constant destination alpha, its
    // value the low byte. Records with no registers (all zero) leave the writes on.
    if st.pe[0] & 0xFF_FFFF != 0 {
        mat.write_colour = st.pe[0] & 8 != 0;
        mat.write_alpha = st.pe[0] & 0x10 != 0;
        mat.dst_alpha = (st.pe[1] & 0x100 != 0).then_some(st.pe[1] as u8);
    }
    mat
}

/// The projected shadows' material as fitted to the reference (`docs/assets.md`, "The renderer"):
/// flat black at the light set's alpha (127/255), the polygon texture's alpha not applied (86.6%
/// of the shadows' pixels within 4 on frame 2400, the same with it applied).
fn shadow_of(mut mat: Material) -> Material {
    mat.combine = Combine::Vertex;
    mat.layers = [None, None];
    mat
}

/// Floor positions are kept in 64ths of a tile and texture coordinates in 512ths (see
/// `MeshData::uv_shift`); the draw's matrix scales the positions back.
const FLOOR_POSITION_UNIT: f32 = 64.0;
const FLOOR_UV_SHIFT: u8 = 3;

/// The terrain's second layer (a lot-wide colour texture, 256 x 256: an aerial picture of the
/// neighbourhood) is read at the position through the device's texture matrix (the material's
/// uv source 3), which the log does not hold. Fitted from the reference's pixels, as `u = x /
/// TERRAIN_COLOUR_TILES_X + TERRAIN_COLOUR_U0` and `v = y / TERRAIN_COLOUR_TILES_Y +
/// TERRAIN_COLOUR_V0` (x and y in tiles; the lot's road lies at low y, which is the road of the
/// picture, so v runs the other way): the means of the grass match the reference's within 2 of
/// 255 over the lot, but the texture's fine detail does not line up (`docs/assets.md`).
const TERRAIN_COLOUR_TILES_X: f32 = 153.25;
const TERRAIN_COLOUR_TILES_Y: f32 = -147.0;
const TERRAIN_COLOUR_U0: f32 = 0.34;
const TERRAIN_COLOUR_V0: f32 = 0.47;

fn terrain_colour_uv() -> PositionUv {
    // positions are in 64ths of a tile
    PositionUv {
        u: [1.0 / (TERRAIN_COLOUR_TILES_X * FLOOR_POSITION_UNIT), 0.0, TERRAIN_COLOUR_U0],
        v: [0.0, 1.0 / (TERRAIN_COLOUR_TILES_Y * FLOOR_POSITION_UNIT), TERRAIN_COLOUR_V0],
    }
}

/// A floor's strip of float vertices (positions in the lot's frame) as a mesh.
fn floor_mesh(vertices: &[StripVertex]) -> MeshData {
    let mut mesh = MeshData { uv_shift: FLOOR_UV_SHIFT, ..MeshData::default() };
    let fixed = |v: f32, unit: f32| (v * unit).round().clamp(-32768.0, 32767.0) as i16;
    mesh.vertices = vertices
        .iter()
        .map(|&(p, uv, c)| Vertex {
            position: [fixed(p[0], FLOOR_POSITION_UNIT), fixed(p[1], FLOOR_POSITION_UNIT), fixed(p[2], FLOOR_POSITION_UNIT), 0],
            uv: [fixed(uv[0], 4096.0 / f32::from(1u16 << FLOOR_UV_SHIFT)), fixed(uv[1], 4096.0 / f32::from(1u16 << FLOOR_UV_SHIFT))],
            colour: c.to_be_bytes(),
            normal: [0, 0, 127, 0],
        })
        .collect();
    strip_indices(&mut mesh.indices, 0, vertices.len() as u32);
    mesh
}

/// A run-time texture's one level from its logged texels (`Log::texels`), through the palette
/// loaded in its TLUT slot for the paletted formats (GX C4 8, C8 9; the palette's own format from
/// the GXTlutObj).
fn runtime_levels(log: &Log, hash: u64, palette: Option<&TlutLoadRec>) -> Option<Vec<(u32, u32, Vec<u32>)>> {
    let t = log.texels.iter().find(|t| t.hash == hash)?;
    let (w, h) = (u32::from(t.width), u32::from(t.height));
    let texels = match t.format {
        8 | 9 => {
            let p = palette?;
            let kind = if t.format == 8 { Kind::Palette4 } else { Kind::Palette8 };
            let bytes: Vec<u8> = p.entries.iter().flat_map(|e| e.to_be_bytes()).collect();
            texture::decode_indexed(kind, w, h, &t.bytes, &bytes, p.format())?
        }
        5 => texture::decode_level(Kind::Bits16, w, h, &t.bytes)?,
        6 => texture::decode_level(Kind::Bits32, w, h, &t.bytes)?,
        14 => texture::decode_level(Kind::Blocks, w, h, &t.bytes)?,
        _ => return None,
    };
    Some(vec![(w, h, texels)])
}

/// Applies the texture loads before the `n`th to the device's two texture maps.
fn load_until<'l>(loads: &'l [TexLoadRec], next: &mut usize, n: usize, state: &mut [Option<&'l TexLoadRec>; 2]) {
    while *next < n.min(loads.len()) {
        let l = &loads[*next];
        state[l.map as usize & 1] = Some(l);
        *next += 1;
    }
}

/// The textures wanted so far, once each by the hash the log names them.
#[derive(Default)]
struct TextureCache {
    ids: HashMap<u64, Option<TextureId>>,
    /// Run-time textures by their hash and their palette's.
    runtime: HashMap<(u64, u64), Option<TextureId>>,
    list: Vec<Vec<(u32, u32, Vec<u32>)>>,
}

impl Assets {
    /// What the device had bound to its first `layers` texture maps, as the layers read it.
    fn bind_textures(&self, log: &Log, when: (u32, usize), state: &[Option<&TexLoadRec>; 2], layers: usize, cache: &mut TextureCache, info: &mut DrawInfo) -> [Option<TextureRef>; 2] {
        let (tick, at) = when;
        let mut bound: [Option<TextureRef>; 2] = [None, None];
        for slot in 0..layers.min(2) {
            let Some(load) = state[slot] else { continue };
            let image = load.image();
            let x = log.textures.iter().rfind(|x| x.image == image && x.tick <= tick).or_else(|| log.textures.iter().find(|x| x.image == image));
            let id = x.and_then(|x| {
                if self.by_hash.contains_key(&x.hash) {
                    return *cache.ids.entry(x.hash).or_insert_with(|| {
                        self.decode(x.hash).map(|levels| {
                            cache.list.push(levels);
                            TextureId(cache.list.len() as u32 - 1)
                        })
                    });
                }
                // made at run time (a Sim's): its texels are in the log, with the palette its
                // object's TLUT name had loaded by this draw
                let name = u32::from_be_bytes(load.obj[24..28].try_into().unwrap());
                let palette = log.tlut_loads.iter().rfind(|t| t.slot == name && (t.tick, t.strips_before) <= (tick, at));
                let key = (x.hash, palette.map_or(0, |t| fnv1a(&t.entries.iter().flat_map(|e| e.to_be_bytes()).collect::<Vec<u8>>())));
                *cache.runtime.entry(key).or_insert_with(|| {
                    runtime_levels(log, x.hash, palette).map(|levels| {
                        cache.list.push(levels);
                        TextureId(cache.list.len() as u32 - 1)
                    })
                })
            });
            info.textures[slot] = x.and_then(|x| self.by_hash.get(&x.hash)).map(|s| s.name.clone());
            let Some(texture) = id else {
                info.missing_texture = true;
                continue;
            };
            let mode0 = u32::from_be_bytes(load.obj[..4].try_into().unwrap());
            // the minifying filter as the hardware codes it: bit 2 linear, the low two
            // bits the mip levels (0 none, 1 the nearest level, 2 two mixed)
            let min = (mode0 >> 5) & 7;
            let mipmaps = match min & 3 {
                0 => Mipmaps::Off,
                1 => Mipmaps::Nearest,
                _ => Mipmaps::Linear,
            };
            bound[slot] = Some(TextureRef { texture, wrap: (wrap_of(mode0), wrap_of(mode0 >> 2)), linear: min & 4 != 0 || mode0 & 0x10 != 0 && min & 3 == 0, mipmaps });
        }
        bound
    }

    /// The frame drawn by the strips of `tick`.
    pub fn scene(&self, log: &Log, tick: u32) -> Scene {
        let strips: Vec<usize> = (0..log.strips.len()).filter(|&i| log.strips[i].tick == tick).collect();
        let (s0, s1) = (strips[0], strips[strips.len() - 1] + 1);

        // the records by the number of strips before them ("gap k": before strip k), in log order
        let mut mats: BTreeMap<usize, Vec<&MaterialRec>> = BTreeMap::new();
        for m in &log.materials {
            mats.entry(m.strips_before).or_default().push(m);
        }
        let mut lights: BTreeMap<usize, Vec<&LightsRec>> = BTreeMap::new();
        for m in &log.lights {
            lights.entry(m.strips_before).or_default().push(m);
        }
        let mut states: BTreeMap<usize, Vec<&StateRec>> = BTreeMap::new();
        for m in &log.states {
            states.entry(m.strips_before).or_default().push(m);
        }
        // the floors' strips ('g' records) by the number of strips before them, with their vertices
        let vertices_of: HashMap<usize, &Vec<StripVertex>> = log.strip_vertices.iter().map(|(j, v)| (*j, v)).collect();
        let mut floors: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (j, g) in log.other_draws.iter().enumerate().filter(|(_, g)| g.0.tick == tick) {
            floors.entry(g.1).or_default().push(j);
        }
        let mut views: BTreeMap<usize, Vec<(Mat3x4, u32)>> = BTreeMap::new();
        let mut worlds: BTreeMap<usize, Vec<(Mat3x4, u32)>> = BTreeMap::new();
        let mut projections: BTreeMap<usize, Vec<([f32; 6], u32)>> = BTreeMap::new();
        for m in &log.matrices {
            if m.projection {
                if m.id == 0 {
                    projections.entry(m.strips_before).or_default().push((m.values[..6].try_into().unwrap(), m.tick));
                }
            } else if m.id == 0xF0 {
                views.entry(m.strips_before).or_default().push((mat3x4(&m.values), m.tick));
            } else if m.id == 0xF1 {
                worlds.entry(m.strips_before).or_default().push((mat3x4(&m.values), m.tick));
            }
        }
        let plus_of: HashMap<usize, &SkinVerticesRec> = log.skin_vertices.iter().map(|v| (v.skin, v)).collect();
        // the skinned draws' '$' records (the bones) by the number of strips before them: a
        // strip's own is in the gap after it
        let mut skins_at: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, k) in log.skins.iter().enumerate().filter(|(_, k)| k.tick == tick) {
            skins_at.entry(k.strips_before).or_default().push(i);
        }

        // the device's state, carried from the log's start: a record before strip k + 1 and after
        // strip k is the draw of strip k setting itself up, so a draw takes the first such record
        // of its tick, and the state before it otherwise
        let mut texture_state: [Option<&TexLoadRec>; 2] = [None, None];
        let (mut view, mut world, mut projection): (Option<Mat3x4>, Option<Mat3x4>, Option<[f32; 6]>) = (None, None, None);
        let mut main_projection: Option<[f32; 6]> = None;
        let mut cache = TextureCache::default();
        let mut next_load = 0;
        let mut mesh_keys: HashMap<(usize, bool, u64), Option<usize>> = HashMap::new();
        let mut meshes: Vec<MeshData> = Vec::new();
        let (mut last_material, mut last_lights): (Option<&MaterialRec>, Option<&LightsRec>) = (None, None);
        let (mut draws, mut drawn, mut all) = (Vec::new(), Vec::new(), Vec::new());
        let mut camera_view: Option<Mat3x4> = None;

        for k in 0..s1 {
            if let Some(v) = views.get(&k) {
                view = v.last().map(|x| x.0);
            }
            if let Some(w) = worlds.get(&k) {
                world = w.last().map(|x| x.0);
            }
            if let Some(p) = projections.get(&k) {
                projection = p.last().map(|x| x.0);
            }
            // the texture loads up to `n` (a floor's strip counts the loads before it)
            if k >= s0 {
                // the floors' materials are the gap's, one each in order, when the gap has as
                // many (the first gap does: the mask passes and the later floors have none)
                let gap_floors = floors.get(&k).map_or(&[][..], |v| &v[..]);
                let gap_materials: Vec<&MaterialRec> = mats.get(&k).map_or(Vec::new(), |v| v.iter().filter(|m| m.tick == tick).copied().collect());
                for (i, &j) in gap_floors.iter().enumerate() {
                    let (g, _, ld) = &log.other_draws[j];
                    load_until(&log.tex_loads, &mut next_load, *ld, &mut texture_state);
                    let Some(vertices) = vertices_of.get(&j).filter(|_| g.words[0] >> 24 == 0x2A) else {
                        let info = DrawInfo { class: Class::Other, strip: k, model: format!("g{j}"), missing_texture: false, textures: [None, None], flags: [0; 2], unsupported: None, skin: None };
                        all.push(info);
                        continue;
                    };
                    // its own state: the first applied after its record (both counts place it)
                    let st = states.get(&k).and_then(|v| v.iter().find(|s| s.tick == tick && s.others_before == j + 1));
                    let masks = st.is_some_and(|s| s.pe[0] & 0xFF_FFFF != 0 && s.pe[0] & 8 == 0);
                    let mut info = DrawInfo { class: if masks { Class::FloorMask } else { Class::Floor }, strip: k, model: format!("g{j}"), missing_texture: false, textures: [None, None], flags: [0; 2], unsupported: None, skin: None };
                    let own = gap_materials.get(i).filter(|_| gap_materials.len() >= gap_floors.len());
                    let bound = self.bind_textures(log, (tick, k + 1), &texture_state, own.map_or(1, |m| m.layers as usize), &mut cache, &mut info);
                    let mut material = match own {
                        Some(m) => material_of(m, true, bound).0,
                        None => Material { combine: Combine::Modulate { unlit: false }, layers: [bound[0], None], blend: Blend::Opaque, cull: Cull::None, depth_test: Some(Compare::LEQUAL), depth_write: true, alpha_test: None, write_colour: true, write_alpha: true, dst_alpha: None, layer1_from_position: None, sphere: None },
                    };
                    if material.layers[1].is_some() {
                        material.layer1_from_position = Some(terrain_colour_uv());
                    }
                    if let Some(st) = st {
                        material = with_state(material, st);
                    }
                    meshes.push(floor_mesh(vertices));
                    let scale = 1.0 / FLOOR_POSITION_UNIT;
                    let world = [scale, 0.0, 0.0, 0.0, 0.0, scale, 0.0, 0.0, 0.0, 0.0, scale, 0.0];
                    let Some(view) = view.or(camera_view) else { continue };
                    camera_view = camera_view.or(Some(view));
                    draws.push(Draw { mesh: MeshId(meshes.len() as u32 - 1), world, bones: None, projection: None, viewport: None, view: Some(view), material, lighting: Lighting::VertexColours, id: drawn.len() as u32 + 1 });
                    drawn.push(info.clone());
                    all.push(info);
                }
            }
            load_until(&log.tex_loads, &mut next_load, log.tex_loads.partition_point(|l| l.strips_before <= k), &mut texture_state);
            if k < s0 {
                continue;
            }
            let m = mats.get(&k).and_then(|v| v.iter().rev().find(|m| m.tick == tick).copied()).or(last_material);
            let lr = lights.get(&k).and_then(|v| v.iter().rev().find(|m| m.tick == tick).copied()).or(last_lights);
            (last_material, last_lights) = (m, lr);
            let first_of = |gap: Option<&Vec<(Mat3x4, u32)>>| gap.and_then(|v| v.iter().find(|x| x.1 == tick)).map(|x| x.0);
            let use_view = first_of(views.get(&(k + 1))).or(view);
            let use_world = first_of(worlds.get(&(k + 1))).or(world);
            let use_projection = projections.get(&(k + 1)).and_then(|v| v.iter().find(|x| x.1 == tick)).map(|x| x.0).or(projection);
            main_projection = main_projection.or(projection).or(use_projection);
            let strip: &StripRec = &log.strips[k];
            let (class, part) = self.identify(strip, k, lr, use_projection, main_projection, &skins_at);
            let mut info = DrawInfo { class, strip: k, model: part.map(|p| p.record.name.clone()).unwrap_or_default(), missing_texture: false, textures: [None, None], flags: [0; 2], unsupported: None, skin: None };
            if let Some(part) = part.filter(|_| class.drawn()) {
                // a blended part is drawn through the bones its '$' record logged
                let skin = part.skinned().then(|| skins_at.get(&(k + 1)).and_then(|v| v.iter().copied().find(|&i| log.skins[i].count == strip.words[1]))).flatten();
                let world = if part.skinned() { Some(IDENTITY) } else { use_world };
                let (Some(m), Some(lr), Some(view), Some(world)) = (m, lr, use_view, world) else {
                    info.class = Class::Unidentified;
                    all.push(info);
                    continue;
                };
                if part.skinned() && skin.is_none() {
                    info.class = Class::Unidentified;
                    all.push(info);
                    continue;
                }
                info.skin = skin;
                let plus = skin.and_then(|i| plus_of.get(&i)).copied();
                let key = (part.batch as *const SubModel as usize, class == Class::DisplayList, plus.map_or(0, plus_key));
                let mesh_index = *mesh_keys.entry(key).or_insert_with(|| {
                    mesh_of(part.batch, class == Class::DisplayList)
                        .and_then(|mesh| match plus {
                            Some(p) => skinned_mesh(mesh, p),
                            None => (!part.skinned()).then_some(mesh),
                        })
                        .map(|mesh| {
                            meshes.push(mesh);
                            meshes.len() - 1
                        })
                });
                let Some(mesh_index) = mesh_index else {
                    info.class = Class::Unidentified;
                    all.push(info);
                    continue;
                };
                let bound = self.bind_textures(log, (tick, k + 1), &texture_state, m.layers as usize, &mut cache, &mut info);
                let lighting = lighting_of(lr);
                let (mut material, unsupported) = material_of(m, matches!(lighting, Lighting::Lit { .. } | Lighting::VertexColours), bound);
                // the draw's own state: the first applied after its record, before the next strip
                let mut draw_viewport = None;
                if let Some(st) = states.get(&(k + 1)).and_then(|v| v.iter().find(|s| s.tick == tick)) {
                    material = with_state(material, st);
                    draw_viewport = inner_viewport(&st.viewport);
                    if class == Class::Shadow {
                        material = shadow_of(material);
                    }
                }
                info.unsupported = unsupported;
                info.flags = [0, 1].map(|i| u32::from_be_bytes(m.layer[i][4..8].try_into().unwrap()));
                camera_view = camera_view.or(Some(view));
                let overlay = class == Class::Overlay;
                draws.push(Draw {
                    mesh: MeshId(mesh_index as u32),
                    world,
                    bones: skin.map(|i| Box::new(log.skins[i].bones)),
                    projection: overlay.then_some(use_projection).flatten(),
                    viewport: overlay.then_some(draw_viewport).flatten(),
                    view: Some(view),
                    material,
                    lighting,
                    id: drawn.len() as u32 + 1,
                });
                drawn.push(info.clone());
            }
            all.push(info);
        }
        let camera = Camera { view: camera_view.unwrap_or(IDENTITY), projection: main_projection.unwrap_or([0.0; 6]) };
        let backdrop = draws.iter().any(|d| d.viewport.is_some()).then_some((PORTRAIT_BACKDROP, PORTRAIT_BACKDROP_COLOUR));
        Scene { frame: Frame { camera, draws, backdrop }, drawn, all, meshes, textures: cache.list }
    }

    /// Which class a strip is and, for a model's, its part.
    fn identify<'a>(&'a self, s: &StripRec, k: usize, lights: Option<&LightsRec>, projection: Option<[f32; 6]>, main: Option<[f32; 6]>, skins_at: &BTreeMap<usize, Vec<usize>>) -> (Class, Option<Part<'a>>) {
        let op = s.words[0] >> 24;
        if op != 0x38 && op != 0x42 {
            return (Class::Other, None);
        }
        let list = op == 0x42;
        let mut parts = self.parts.find(s);
        if list {
            parts.retain(|p| p.batch.list.as_ref().is_some_and(|l| l.2.iter().zip(s.at[6].iter()).all(|(a, b)| a == b)));
        }
        let part = parts.first().copied();
        if projection != main {
            return (Class::Overlay, part);
        }
        let Some(part) = part else {
            return (if skins_at.contains_key(&k) || skins_at.contains_key(&(k + 1)) { Class::Skinned } else { Class::Unidentified }, None);
        };
        // a Sim's projected shadow is a skinned part too (`shadow_adult`) drawn with lighting off
        if lights.is_some_and(|l| l.flags() & 0x4000 != 0) {
            return (Class::Shadow, Some(part));
        }
        if part.skinned() {
            return (Class::Skinned, Some(part));
        }
        (if list { Class::DisplayList } else { Class::RigidStrip }, Some(part))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_record_sets_blend_depth_and_alpha_test() {
        let base = Material { combine: Combine::Vertex, layers: [None, None], blend: Blend::Opaque, cull: Cull::Back, depth_test: None, depth_write: false, alpha_test: None, write_colour: true, write_alpha: true, dst_alpha: None, layer1_from_position: None, sphere: None };
        // as the frames' blended glass: on by flag 0x40, src alpha / inverse, Z <= with writes, alpha >= 5
        let st = StateRec { tick: 0, context: 0, mtx_mode: 0, strips_before: 0, others_before: 0, z: (true, 3, true), alpha: (6, 5, 0, 7, 0), z_before_tex: false, blend: [1, 4, 5, 0], word574: 0, blend_flags: 0x48, cull: 0, pe: [0; 4], tex_mtx: [[0.0; 12]; 2], viewport: [0.0; 6] };
        let m = with_state(base, &st);
        assert_eq!(m.blend, Blend::ALPHA);
        assert_eq!((m.depth_test, m.depth_write, m.cull), (Some(Compare::LEQUAL), true, Cull::None));
        assert_eq!(m.alpha_test, Some(AlphaTest::at_least(5)));
        // blending off by the flag whatever the factors; Z off takes the writes with it; an
        // always-and-always compare is no test
        let st = StateRec { z: (false, 3, true), alpha: (7, 0, 1, 7, 0), blend_flags: 0x08, ..st };
        let m = with_state(base, &st);
        assert_eq!((m.blend, m.depth_test, m.depth_write, m.alpha_test), (Blend::Opaque, None, false, None));
    }

    #[test]
    fn pixel_engine_registers_set_the_write_masks_and_constant_alpha() {
        let base = Material { combine: Combine::Vertex, layers: [None, None], blend: Blend::Opaque, cull: Cull::None, depth_test: None, depth_write: false, alpha_test: None, write_colour: true, write_alpha: true, dst_alpha: None, layer1_from_position: None, sphere: None };
        let st = StateRec { tick: 0, context: 0, mtx_mode: 0, strips_before: 0, others_before: 0, z: (false, 3, false), alpha: (7, 0, 1, 7, 0), z_before_tex: false, blend: [1, 2, 0, 0], word574: 0, blend_flags: 0x148, cull: 0, pe: [0x4100_0215, 0x4200_0100, 0, 0], tex_mtx: [[0.0; 12]; 2], viewport: [0.0; 6] };
        // the mask pass: alpha only, constant alpha 0
        let m = with_state(base, &st);
        assert_eq!((m.write_colour, m.write_alpha, m.dst_alpha), (false, true, Some(0)));
        // an opaque draw: both, with the receivers' constant alpha 0x7F
        let st = StateRec { pe: [0x4100_001C, 0x4200_017F, 0, 0], ..st };
        let m = with_state(base, &st);
        assert_eq!((m.write_colour, m.write_alpha, m.dst_alpha), (true, true, Some(0x7F)));
    }

    #[test]
    fn floor_strips_keep_tile_units_in_fixed_point() {
        let mesh = floor_mesh(&[([1.0, 2.0, 0.0, 0.0], [-18.0, 1.0], 0x80_80_70_80), ([2.0, 2.0, 0.0, 0.0], [-17.0, 1.0], 0), ([1.0, 3.0, 0.0, 0.0], [-18.0, 0.0], 0)]);
        assert_eq!(mesh.vertices[0].position, [64, 128, 0, 0]);
        // 4096ths of 8: -18 is -9216
        assert_eq!(mesh.vertices[0].uv, [-9216, 512]);
        assert_eq!(mesh.vertices[0].colour, [0x80, 0x80, 0x70, 0x80]);
        assert_eq!(mesh.indices, [0, 1, 2]);
    }

    #[test]
    fn skinned_mesh_takes_the_logged_vertices_and_carries_the_heaviest_bone() {
        let mesh = MeshData { vertices: vec![Vertex { uv: [7, 8], ..Vertex::default() }; 3], ..MeshData::default() };
        let mut positions = Vec::new();
        for x in [1i16, -2, 3] {
            positions.extend_from_slice(&x.to_be_bytes());
            positions.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        }
        // vertex 0: bones 1 and 2 tie, so the first (1); vertex 1 has no weight and keeps it; vertex 2: bone 3
        let plus = SkinVerticesRec { skin: 0, count: 3, flags: 1, heaviest: 2, positions, weights: vec![0, 9, 9, 0, 0, 0, 0, 0, 0, 0, 0, 5], normals: [0, 0, 127, 0].repeat(3), sent: Vec::new() };
        let m = skinned_mesh(mesh, &plus).unwrap();
        assert_eq!(m.vertices[1].position[0], -2);
        assert_eq!(m.vertices[2].uv, [7, 8]);
        assert_eq!(m.vertices[0].normal, [0, 0, 127, 0]);
        assert_eq!(m.skin.iter().map(|w| w.heaviest).collect::<Vec<_>>(), [1, 1, 3]);
        // a record for another vertex count is not this mesh
        assert!(skinned_mesh(MeshData::default(), &plus).is_none());
    }

    #[test]
    fn strips_become_triangles_all_facing_one_way() {
        let mut out = Vec::new();
        strip_indices(&mut out, 10, 5);
        assert_eq!(out, [10, 11, 12, 12, 11, 13, 12, 13, 14]);
        out.clear();
        strip_indices(&mut out, 0, 2);
        assert!(out.is_empty());
    }

    #[test]
    fn colours_go_through_bytes_as_the_game_makes_them() {
        assert_eq!(byte(0.5), 128.0 / 255.0);
        assert_eq!(byte(2.0), 1.0);
        assert_eq!(byte(-1.0), 0.0);
    }
}
