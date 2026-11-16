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
use housefire::tree_log::{LightsRec, Log, MaterialRec, StripRec, TexLoadRec};
use std::collections::{BTreeMap, HashMap};

/// The alpha below which a textured pixel is not drawn. A stand-in: the device's alpha compare
/// (`ERC_ApplyState`'s block at context +0x248) is not in the log; 0.3 to 0.5 fit the same.
pub const ALPHA_REF: f32 = 0.5;

/// A layer's flag (the low byte of its flag word, 0x08 for most materials) that goes with the
/// translucent ones (windows, the aquarium, the bottle lamp, water): drawn blended by source
/// alpha, with no alpha test. Read from the log's material records, and checked by the pixels
/// those draws cover (`docs/assets.md`, "The renderer"); the blend state itself is not logged.
pub const LAYER_BLENDED: u8 = 0x40;

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
    /// Another command (the floors' strips, 'g' records).
    Other,
    /// A strip whose vertices are not in `models.arc` (made at run time) or whose model is not read.
    Unidentified,
}

impl Class {
    pub const ALL: [Class; 7] = [Class::RigidStrip, Class::DisplayList, Class::Skinned, Class::Shadow, Class::Overlay, Class::Other, Class::Unidentified];

    pub fn name(self) -> &'static str {
        match self {
            Class::RigidStrip => "rigid model strip",
            Class::DisplayList => "display-list model draw",
            Class::Skinned => "skinned (not drawn: R2)",
            Class::Shadow => "lighting off, shadows (not drawn: R2)",
            Class::Overlay => "other projection, interface (not drawn)",
            Class::Other => "other command, floors (not drawn)",
            Class::Unidentified => "unidentified",
        }
    }

    /// Whether R1 draws it.
    pub fn drawn(self) -> bool {
        matches!(self, Class::RigidStrip | Class::DisplayList)
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
}

/// A texture of `textures.arc` or `datasets.arc`: where its bytes are.
struct TextureSource {
    name: String,
    in_datasets: bool,
    /// The LFXT's offset in its archive's data.
    at: usize,
}

/// The game's files a frame is made from.
pub struct Assets {
    parts: PartIndex,
    textures_arc: Vec<u8>,
    datasets_arc: Vec<u8>,
    /// FNV-1a of a texture's first level, as the observer hashes the images it sees.
    by_hash: HashMap<u64, TextureSource>,
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
        for e in arc::entries(&textures_arc).unwrap_or_default() {
            let b = arc::bytes(&textures_arc, &e);
            if let Some(t) = texture::read(b) {
                let first = (t.width as usize * t.height as usize * t.kind.bits() as usize) / 8;
                by_hash.entry(fnv1a(&b[t.texels.0..t.texels.0 + first])).or_insert(TextureSource { name: t.name.clone(), in_datasets: false, at: e.offset as usize });
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
                    by_hash.entry(fnv1a(&b[at + t.texels.0..at + t.texels.0 + first])).or_insert(TextureSource { name: format!("{}/{}", e.name, t.name), in_datasets: true, at: e.offset as usize + at });
                    p = at + end;
                } else {
                    p = at + 4;
                }
            }
        }
        Assets { parts, textures_arc, datasets_arc, by_hash }
    }

    /// A texture's mip levels as RGBA (the levels whole tiles of the format hold).
    fn decode(&self, hash: u64) -> Option<Vec<(u32, u32, Vec<u32>)>> {
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
fn mesh_of(sub: &SubModel, display_list: bool) -> Option<MeshData> {
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

fn mat3x4(v: &[f32]) -> Mat3x4 {
    v[..12].try_into().unwrap()
}

fn apply(m: &Mat3x4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r * 4] * p[0] + m[r * 4 + 1] * p[1] + m[r * 4 + 2] * p[2] + m[r * 4 + 3])
}

/// A colour's channels in 0..1 as the game's conversion makes them bytes, back as 0..1.
fn byte(v: f32) -> f32 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() / 255.0
}

/// The light set the device was given (`ERC_SetLights`, the recomp's `erc_lights.cpp`): the
/// ambient and each light's colour times its tint as bytes; directionals turned by the device's
/// matrix, points moved into view space by the view the device had.
fn lighting_of(l: &LightsRec) -> Lighting {
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
/// them (`erc_material.cpp`). The state a draw is made in beyond that (blend of a one-layer
/// material, depth, cull, alpha compare) is not in the log: the defaults are used.
fn material_of(m: &MaterialRec, lit: bool, bound: [Option<TextureRef>; 2]) -> (Material, Option<&'static str>) {
    let l = |i: usize, o: usize| m.layer[i][o];
    let (mode, blend, by, enabled, factor) = (l(1, 0x10), l(1, 0x11), l(1, 0x12), l(1, 0x13), l(1, 0x14));
    let sphere = m.layers == 2 && l(1, 0x16) == 2;
    let mut unsupported = None;
    let two = m.layers == 2 && bound[1].is_some() && lit && !sphere;
    let mut mat = Material { combine: Combine::Vertex, layers: [bound[0], None], blend: Blend::Opaque, cull: Cull::None, depth_test: true, depth_write: true, alpha_ref: None, sphere: None };
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
                mat.blend = Blend::Add;
            }
            (2, 2) => unsupported = Some("added by the game's table"),
            (1, 1) => {
                mat.combine = Combine::Over { by_texture_alpha: false, unlit_first };
                mat.blend = Blend::Alpha;
            }
            (1, 0) => {
                mat.combine = Combine::Over { by_texture_alpha: true, unlit_first };
                mat.blend = Blend::Alpha;
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
                mat.blend = Blend::Alpha;
            } else {
                mat.alpha_ref = Some(ALPHA_REF);
            }
            if sphere && lit && bound[1].is_some() {
                mat.sphere = Some(sphere_weight);
                mat.layers = bound;
            }
        }
    }
    (mat, unsupported)
}

impl Assets {
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
        let mut loads: BTreeMap<usize, Vec<&TexLoadRec>> = BTreeMap::new();
        for m in &log.tex_loads {
            loads.entry(m.strips_before).or_default().push(m);
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
        let skinned_at: std::collections::BTreeSet<usize> = log.skins.iter().map(|k| k.strips_before).collect();

        // the device's state, carried from the log's start: a record before strip k + 1 and after
        // strip k is the draw of strip k setting itself up, so a draw takes the first such record
        // of its tick, and the state before it otherwise
        let mut texture_state: [Option<&TexLoadRec>; 2] = [None, None];
        let (mut view, mut world, mut projection): (Option<Mat3x4>, Option<Mat3x4>, Option<[f32; 6]>) = (None, None, None);
        let mut main_projection: Option<[f32; 6]> = None;
        let mut hash_textures: HashMap<u64, Option<TextureId>> = HashMap::new();
        let mut texture_list: Vec<Vec<(u32, u32, Vec<u32>)>> = Vec::new();
        let mut mesh_keys: HashMap<(usize, bool), Option<usize>> = HashMap::new();
        let mut meshes: Vec<MeshData> = Vec::new();
        let (mut last_material, mut last_lights): (Option<&MaterialRec>, Option<&LightsRec>) = (None, None);
        let (mut draws, mut drawn, mut all) = (Vec::new(), Vec::new(), Vec::new());
        let mut camera_view: Option<Mat3x4> = None;

        for k in 0..s1 {
            for l in loads.get(&k).into_iter().flatten() {
                texture_state[l.map as usize & 1] = Some(l);
            }
            if let Some(v) = views.get(&k) {
                view = v.last().map(|x| x.0);
            }
            if let Some(w) = worlds.get(&k) {
                world = w.last().map(|x| x.0);
            }
            if let Some(p) = projections.get(&k) {
                projection = p.last().map(|x| x.0);
            }
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
            let (class, part) = self.identify(strip, k, lr, use_projection, main_projection, &skinned_at);
            let mut info = DrawInfo { class, strip: k, model: part.map(|p| p.record.name.clone()).unwrap_or_default(), missing_texture: false, textures: [None, None], flags: [0; 2], unsupported: None };
            if class.drawn() {
                let part = part.unwrap();
                let (Some(m), Some(lr), Some(view), Some(world)) = (m, lr, use_view, use_world) else {
                    info.class = Class::Unidentified;
                    all.push(info);
                    continue;
                };
                let key = (part.batch as *const SubModel as usize, class == Class::DisplayList);
                let mesh_index = *mesh_keys.entry(key).or_insert_with(|| {
                    mesh_of(part.batch, class == Class::DisplayList).map(|mesh| {
                        meshes.push(mesh);
                        meshes.len() - 1
                    })
                });
                let Some(mesh_index) = mesh_index else {
                    info.class = Class::Unidentified;
                    all.push(info);
                    continue;
                };
                let mut bound: [Option<TextureRef>; 2] = [None, None];
                for slot in 0..(m.layers as usize).min(2) {
                    let Some(load) = texture_state[slot] else { continue };
                    let image = load.image();
                    let x = log.textures.iter().rfind(|x| x.image == image && x.tick <= tick).or_else(|| log.textures.iter().find(|x| x.image == image));
                    let id = x.and_then(|x| {
                        *hash_textures.entry(x.hash).or_insert_with(|| {
                            self.decode(x.hash).map(|levels| {
                                texture_list.push(levels);
                                TextureId(texture_list.len() as u32 - 1)
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
                let lighting = lighting_of(lr);
                let (material, unsupported) = material_of(m, matches!(lighting, Lighting::Lit { .. } | Lighting::VertexColours), bound);
                info.unsupported = unsupported;
                info.flags = [0, 1].map(|i| u32::from_be_bytes(m.layer[i][4..8].try_into().unwrap()));
                camera_view = camera_view.or(Some(view));
                draws.push(Draw { mesh: MeshId(mesh_index as u32), world, view: Some(view), material, lighting, id: drawn.len() as u32 + 1 });
                drawn.push(info.clone());
            }
            all.push(info);
        }
        let camera = Camera { view: camera_view.unwrap_or(IDENTITY), projection: main_projection.unwrap_or([0.0; 6]) };
        Scene { frame: Frame { camera, draws }, drawn, all, meshes, textures: texture_list }
    }

    /// Which class a strip is and, for a model's, its part.
    fn identify<'a>(&'a self, s: &StripRec, k: usize, lights: Option<&LightsRec>, projection: Option<[f32; 6]>, main: Option<[f32; 6]>, skinned_at: &std::collections::BTreeSet<usize>) -> (Class, Option<Part<'a>>) {
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
            return (if skinned_at.contains(&k) || skinned_at.contains(&(k + 1)) { Class::Skinned } else { Class::Unidentified }, None);
        };
        if part.skinned() {
            return (Class::Skinned, Some(part));
        }
        if lights.is_some_and(|l| l.flags() & 0x4000 != 0) {
            return (Class::Shadow, Some(part));
        }
        (if list { Class::DisplayList } else { Class::RigidStrip }, Some(part))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
