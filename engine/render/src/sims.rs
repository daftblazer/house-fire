//! The Sims drawn from the engine's own state (`docs/assets.md`, "The Sims from the engine's
//! state"): each Sim's parts from its description and the part tables, its bones from its
//! animation (`housefire::figure`), the skinned parts blended on the GPU through them, the
//! shadows, and the portrait of one Sim in the corner.
//!
//! What still comes from the log: the Sims' run-time textures (the one 128 x 128 image the
//! game makes of a Sim's skin, face, hair and clothes: `SimSkins`) and, where the engine does
//! not know it, a Sim's heading.

use crate::engine_scene::{lighting_for, shader_material, shadow_material, shadow_view, Caches, Made, SHADOW_SHEAR_OBJECT};
use crate::frame::*;
use crate::scene::{mesh_of, Assets, Class, DrawInfo, Scene, PORTRAIT_BACKDROP, PORTRAIT_BACKDROP_COLOUR};
use housefire::anim::Catalog;
use housefire::drawlist::{Kind, Models};
use housefire::figure::{self, Cast, Rigs};
use housefire::lighting::LotLights;
use housefire::outfit::{Description, Slot};
use housefire::shader::{name_hash, Shader};
use std::collections::HashMap;

/// A Sim as a frame needs to know it, from the engine's world.
#[derive(Clone, Debug)]
pub struct Figure {
    pub id: i16,
    pub description: Description,
    /// Where it stands on the lot, in tiles.
    pub place: [f32; 2],
    /// Which way it faces, in eighths of a turn about the vertical.
    pub heading: u8,
    /// Its animation's id, how far along it is (frames) and whether it plays backward.
    pub play: Option<(u32, f32, bool)>,
    /// Whether it holds something in its hand (the carrying loop plays on its arm).
    pub carrying: bool,
    /// Its mood motive (`figure::FACE_MOTIVE`), the weight of its face track times 100.
    pub mood: f32,
}

/// The parts of a Sim's outfit in the order the house's draws take them (the records of the
/// log's frame 2400: hair, face, upper body, lower body, shoes, then the accessory).
const HOUSE_ORDER: [Slot; 8] = [Slot::Head, Slot::Eyes, Slot::Nose, Slot::Mouth, Slot::Upper, Slot::Lower, Slot::Shoes, Slot::Accessory];
/// The portrait's parts and their order (the same records: the face first, then the hair and the
/// jacket, then the glasses; the legs and feet are out of its picture).
const PORTRAIT_ORDER: [Slot; 6] = [Slot::Eyes, Slot::Nose, Slot::Mouth, Slot::Head, Slot::Upper, Slot::Accessory];

/// The shader a Sim's parts name: one layer whose texture is the run-time image of the Sim's
/// skin, face, hair and clothes (`SimSkins`), whatever the texture of that name is on the disc.
const SIM_BLANK_SHADER: &str = "_______BLANK";

/// The light a Sim in a room has besides its room's ambient and four lights: one white
/// directional of colour 0.4 (the logged Sims' sets all have it, whatever the room), its
/// direction to the light in world space. Measured on the logged set of frame 2400 (the view's
/// turn undone): (-0.7532, -0.2926, -0.5892).
pub const SIM_FILL: (f32, [f32; 3]) = (0.4, [-0.7531519, -0.2926267, -0.589179]);

/// The portrait's projection, viewport and lights, as the log's 57 overlay draws have them
/// (every one the same): 1:1 about the head, a 54 x 56 pixel corner, ambient 0.2 and two
/// directionals in the portrait's own view.
pub const PORTRAIT_PROJECTION: [f32; 6] = [8.144346, 0.0, 8.144346, 0.0, -0.001001001, -0.5005005];
pub const PORTRAIT_VIEWPORT: Viewport = Viewport { x: 49.92, y: 346.304, width: 54.08, height: 55.552 };
const PORTRAIT_AMBIENT: [f32; 3] = [0.2; 3];
const PORTRAIT_LIGHTS: [([f32; 3], [f32; 3]); 2] = [([0.1764706, 0.1764706, 0.2509804], [-0.69713944, 0.16731332, 0.697139]), ([0.2509804, 0.2509804, 0.22352941], [0.69713914, 0.16731332, 0.6971393])];
/// The portrait's camera in the Sim's own frame (x right, y forward, z up), looking back at it:
/// its distance in front, its height, the pitch up and a slight yaw. From the logged view of
/// frame 2400 (eye (24.5010, 17.8027, 1.6014) for the Sim at (24.5, 19.0) facing -y, so 1.1973
/// in front and 1.6014 up; pitch 0.0835 rad, yaw 0.0008), the yaw then refined against the
/// portrait's own pixels with the Sim's layered pose (a coordinate descent on the portrait's
/// pixels beyond 4: 404 at the logged yaw, 120 at -0.000075; distance, height and pitch stay the
/// logged ones; the logged eye is 0.001 aside of the Sim's own x, which the yaw absorbs).
pub const PORTRAIT_DISTANCE: f32 = 1.1973;
pub const PORTRAIT_HEIGHT: f32 = 1.6014;
pub const PORTRAIT_PITCH: f32 = 0.0835;
pub const PORTRAIT_YAW: f32 = -0.000075;

/// The portrait's view of a Sim at `place` facing `heading` (`figure::root`'s turn).
pub fn portrait_view(place: [f32; 2], heading: u8) -> Mat3x4 {
    let a = f32::from(heading) * std::f32::consts::FRAC_PI_4;
    let (s, c) = a.sin_cos();
    // the Sim's forward (its local +y) and right in the world
    let forward = [-s, c];
    let (dist, height, pitch, yaw) = (PORTRAIT_DISTANCE, PORTRAIT_HEIGHT, PORTRAIT_PITCH, PORTRAIT_YAW);
    let right0 = [-c, -s];
    let eye = [place[0] + forward[0] * dist, place[1] + forward[1] * dist, height];
    let (sp, cp) = pitch.sin_cos();
    // the camera looks back along -forward, tilted up and turned by the yaw
    let (sy, cy) = yaw.sin_cos();
    let back = [-forward[0], -forward[1]];
    let flat = [back[0] * cy - back[1] * sy, back[0] * sy + back[1] * cy];
    let look = [flat[0] * cp, flat[1] * cp, sp];
    let right = [right0[0] * cy - right0[1] * sy, right0[0] * sy + right0[1] * cy, 0.0];
    let up = [right[1] * look[2] - right[2] * look[1], right[2] * look[0] - right[0] * look[2], right[0] * look[1] - right[1] * look[0]];
    let back = look.map(|v| -v);
    let rows = [right, up, back];
    let mut m = [0.0; 12];
    for (r, row) in rows.iter().enumerate() {
        m[r * 4..r * 4 + 3].copy_from_slice(row);
        m[r * 4 + 3] = -(row[0] * eye[0] + row[1] * eye[1] + row[2] * eye[2]);
    }
    m
}

/// The Sims' run-time textures: made by the engine from the disc (`housefire::sim_texture`,
/// which equals the log's byte for byte) or, with none of the disc's run-length textures
/// attached, as the log has them: the image of each outfit part name the log drew with one, its
/// levels and how the draw sampled it.
#[derive(Default)]
pub struct SimSkins {
    by_part: HashMap<String, Skin>,
    /// `rletextu.arc`, when the engine makes the textures.
    rle: Option<Vec<u8>>,
}

/// How a Sim's run-time texture is sampled: clamped, bilinear, one level (every logged draw's).
const SKIN_SAMPLER: (Wrap, Wrap, bool, Mipmaps) = (Wrap::Clamp, Wrap::Clamp, true, Mipmaps::Off);

/// A Sim's looks as a texture's key: gender, age and the saved looks.
type LooksKey = (bool, bool, [u8; 18]);

/// An image's levels (width, height, RGBA texels) and the sampler of its draw.
type Skin = (Vec<(u32, u32, Vec<u32>)>, TextureRef);

impl SimSkins {
    /// From a log-mode scene: the first run-time texture (one with no name) each Sim part's
    /// draw has.
    pub fn from_scene(scene: &Scene) -> SimSkins {
        let mut by_part = HashMap::new();
        for d in &scene.frame.draws {
            let info = &scene.drawn[d.id as usize - 1];
            let Some(t) = d.material.layers[0] else { continue };
            if housefire::drawlist::is_sims(&info.model) && !info.model.starts_with("shadow_") && info.textures[0].is_none() {
                by_part.entry(info.model.to_string()).or_insert_with(|| (scene.textures[t.texture.0 as usize].clone(), t));
            }
        }
        SimSkins { by_part, rle: None }
    }

    /// The engine makes the Sims' textures from the disc, unless HF_LOG_SKINS is set (the log's
    /// stay, to compare).
    pub fn with_disc(mut self, disc: &housefire::disc::Disc) -> SimSkins {
        if std::env::var_os("HF_LOG_SKINS").is_none() {
            self.rle = disc.read("rletextu.arc");
        }
        self
    }

    /// The engine's texture of a Sim of this description: its levels, `None` when a layer of it
    /// is not on the disc or the engine makes none.
    fn engine_levels(&self, tables: &housefire::outfit::PartTables, d: &Description) -> Option<Vec<(u32, u32, Vec<u32>)>> {
        let arc = self.rle.as_deref()?;
        let stack = housefire::sim_texture::layer_stack(tables, d, housefire::sim_texture::Conditions { tinted_clothes: true });
        let texture = housefire::sim_texture::reduce(&housefire::sim_texture::pictures(arc, &stack)?);
        let side = housefire::sim_texture::SIDE as u32;
        Some(vec![(side, side, texture.decode()?)])
    }
}

/// What a Sim's frame is made of, in the original's order.
#[derive(Default)]
pub struct SimDraws {
    /// The Sims' parts in the lot's view.
    pub house: Vec<Made>,
    /// Their shadows, with the room each falls in.
    pub shadows: Vec<(usize, Made)>,
    /// The portrait's draws (its own view, projection and viewport).
    pub portrait: Vec<Made>,
    /// What the portrait's Sim draws once more in the lot's view after its portrait, with the
    /// portrait's light (the original's own leak: its accessory).
    pub leak: Vec<Made>,
}

/// The disc's data the Sims are made from, and what they stand on.
pub struct Wardrobe<'a> {
    pub rigs: &'a Rigs,
    pub cast: &'a Cast,
    pub catalog: &'a Catalog,
    pub skins: &'a SimSkins,
    pub models: &'a Models,
    pub shaders: &'a housefire::fasthash::FastMap<u32, Shader>,
}

/// Per-run caches of the Sims' meshes and textures.
#[derive(Default)]
pub struct SimCaches {
    meshes: HashMap<usize, Option<MeshId>>,
    skins: HashMap<String, TextureRef>,
    /// The engine's textures by the looks that make them (`None`: it made none).
    engine_skins: HashMap<LooksKey, Option<TextureRef>>,
}

/// A skinned batch as a mesh: the model's own positions and normals with the blend weights it
/// keeps (4 bytes a vertex), the bone that turns a normal the first of the largest weights (the
/// one before when a vertex has none, as the original's loop carries it).
fn skinned_mesh_of(sub: &housefire::model::SubModel) -> Option<MeshData> {
    let mut mesh = mesh_of(sub, false)?;
    if sub.blend.len() != mesh.vertices.len() * 4 {
        return None;
    }
    let mut heaviest = 0u8;
    for &weights in sub.blend.as_chunks::<4>().0 {
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

/// The light a Sim in the house has: the four lights of the grid point of the tile it stands in
/// (an object's are the nearest whole tile's: the logged sets of both Sims of frame 2400 are the
/// tile's, which a position on a tile's half (24.5) tells apart), and the fill `SIM_FILL`.
fn house_lighting(lights: &LotLights, place: [f32; 2], view: &Mat3x4) -> Lighting {
    let grid = [place[0].floor(), place[1].floor()];
    let Lighting::Lit { ambient, mut directionals, points } = lighting_for(&lights.at_in(place, grid), view, None) else { unreachable!("lighting_for lights") };
    let to_light = std::array::from_fn(|r| view[r * 4] * SIM_FILL.1[0] + view[r * 4 + 1] * SIM_FILL.1[1] + view[r * 4 + 2] * SIM_FILL.1[2]);
    directionals.push(Directional { colour: [SIM_FILL.0; 3], to_light });
    Lighting::Lit { ambient, directionals, points }
}

/// The light of the portrait's draws.
fn portrait_lighting() -> Lighting {
    Lighting::Lit {
        ambient: PORTRAIT_AMBIENT,
        directionals: PORTRAIT_LIGHTS.iter().map(|&(colour, to_light)| Directional { colour, to_light }).collect(),
        points: Vec::new(),
    }
}

impl Figure {
    /// What the Sim does, as far as its pose goes.
    fn stance(&self) -> figure::Stance {
        figure::Stance { action: self.play, carrying: self.carrying, mood: self.mood, adult: self.description.adult }
    }

    /// Its bones in the form the renderer takes, at `heading`.
    fn bone_rows(&self, w: &Wardrobe, heading: u8) -> Vec<Mat3x4> {
        let nodes = w.rigs.of(&self.description);
        let masks = figure::Masks::new(nodes.len());
        let tracks = figure::tracks(w.catalog, &self.stance(), &masks);
        figure::bones(nodes, &tracks, &figure::root(self.place[0], self.place[1], heading)).iter().map(figure::rows).collect()
    }
}

impl Assets {
    /// The Sims' draws for `figures`, standing in the lot lit by `lights`, seen through `view`
    /// (`sun`: the direction the sun shines in, for the shadows outdoors); the portrait of
    /// `portrait` (a Sim's id) when it is on the lot.
    #[allow(clippy::too_many_arguments)]
    pub fn engine_sims(&self, scene: &mut Scene, caches: &mut Caches, cache: &mut SimCaches, w: &Wardrobe, figures: &[Figure], lights: &LotLights, sun: [f32; 3], view: Mat3x4, portrait: Option<i16>) -> SimDraws {
        let mut out = SimDraws::default();
        for f in figures {
            let bones = f.bone_rows(w, f.heading);
            let outfit = w.cast.tables.dress(&f.description);
            let records: Vec<(Slot, usize)> = Slot::ALL.iter().filter_map(|&s| Some((s, w.models.parts.record_by_id(outfit.part(s))?))).collect();
            let skin = Self::skin_of(scene, cache, w, f, &records);
            let room = lights.room_of_place(f.place[0], f.place[1]);
            let house_light = house_lighting(lights, f.place, &view);
            let order = |slots: &[Slot]| -> Vec<usize> { slots.iter().filter_map(|s| records.iter().find(|r| r.0 == *s).map(|r| r.1)).collect() };
            for r in order(&HOUSE_ORDER) {
                self.sim_part_draws(scene, caches, cache, w, r, &bones, skin, &house_light, view, None, &mut out.house);
            }
            if portrait == Some(f.id) {
                let pview = portrait_view(f.place, f.heading);
                let plight = portrait_lighting();
                let overlay = Some((PORTRAIT_PROJECTION, PORTRAIT_VIEWPORT));
                for r in order(&PORTRAIT_ORDER) {
                    self.sim_part_draws(scene, caches, cache, w, r, &bones, skin, &plight, pview, overlay, &mut out.portrait);
                }
                for r in order(&[Slot::Accessory]) {
                    self.sim_part_draws(scene, caches, cache, w, r, &bones, skin, &plight, view, None, &mut out.leak);
                }
                scene.frame.backdrop = Some((PORTRAIT_BACKDROP, PORTRAIT_BACKDROP_COLOUR));
            }
            // the shadow: the model of its own (`shadow_adult`), blended through the same bones,
            // flat on the ground (indoors) or sheared along the sun (outdoors)
            let shadow_name = if f.description.adult { "shadow_adult" } else { "shadow_child" };
            if let Some(r) = w.models.parts.record_by_id(name_hash(shadow_name)) {
                let sview = if room == 0 { shadow_view(&view, sun, SHADOW_SHEAR_OBJECT) } else { shadow_view(&view, [0.0; 3], 1.0) };
                let mut parts = Vec::new();
                self.sim_part_draws(scene, caches, cache, w, r, &bones, None, &Lighting::Black, sview, None, &mut parts);
                for mut m in parts {
                    m.draw.material = shadow_material();
                    m.draw.lighting = Lighting::Black;
                    m.info.class = Class::Shadow;
                    out.shadows.push((room, m));
                }
            }
        }
        out
    }

    /// The run-time texture of a Sim's parts: the engine's, made once for each looks; else (the
    /// engine makes none) the log's for the first of its part names the log drew.
    fn skin_of(scene: &mut Scene, cache: &mut SimCaches, w: &Wardrobe, f: &Figure, records: &[(Slot, usize)]) -> Option<TextureRef> {
        let looks: LooksKey = (f.description.male, f.description.adult, f.description.looks);
        let engine_skin = *cache.engine_skins.entry(looks).or_insert_with(|| {
            let levels = w.skins.engine_levels(&w.cast.tables, &f.description)?;
            let (wrap_u, wrap_v, linear, mipmaps) = SKIN_SAMPLER;
            Some(TextureRef { texture: scene.add_texture(levels), wrap: (wrap_u, wrap_v), linear, mipmaps })
        });
        engine_skin.or_else(|| {
            let name = HOUSE_ORDER.iter().filter_map(|s| records.iter().find(|r| r.0 == *s)).map(|r| &*w.models.parts.record(r.1).name).find(|n| w.skins.by_part.contains_key(*n))?;
            let (levels, tex) = &w.skins.by_part[name];
            Some(*cache.skins.entry(name.to_string()).or_insert_with(|| TextureRef { texture: scene.add_texture(levels.clone()), ..*tex }))
        })
    }

    /// The draws of every batch of record `r` through the Sim's `bones`: a batch on one bone
    /// hangs from it (its matrix is the draw's world), a blended one takes its matrices' bones
    /// (up to four) for the GPU's skinning.
    #[allow(clippy::too_many_arguments)]
    fn sim_part_draws(&self, scene: &mut Scene, caches: &mut Caches, cache: &mut SimCaches, w: &Wardrobe, r: usize, bones: &[Mat3x4], skin: Option<TextureRef>, lighting: &Lighting, view: Mat3x4, overlay: Option<([f32; 6], Viewport)>, out: &mut Vec<Made>) {
        for part in w.models.parts.parts_of(r) {
            let batch = part.batch;
            let bone = |slot: u16| bones.get(usize::from(slot)).copied();
            let (mesh, world, matrices) = if part.skinned() {
                let mesh = *cache.meshes.entry(batch as *const _ as usize).or_insert_with(|| skinned_mesh_of(batch).map(|m| scene.add_mesh(m)));
                (mesh, IDENTITY, Some(Box::new(skin_matrices(batch, bone))))
            } else {
                let Some(&(slot, _)) = batch.matrices.first() else { continue };
                let Some(world) = bone(slot) else { continue };
                (caches.mesh(scene, batch, batch.list.is_some()), world, None)
            };
            let Some(mesh) = mesh else { continue };
            let shader = w.shaders.get(&batch.hash);
            let class = if overlay.is_some() { Class::Overlay } else { Class::Skinned };
            let mut info = DrawInfo::new(class, 0, part.record.name.clone());
            let (mut bound, cutout) = self.bind_shader(scene, &mut caches.textures, shader, &mut info);
            // the shader's blank layer names the Sim's own image
            if shader.is_some_and(|sh| sh.name == SIM_BLANK_SHADER) {
                bound[0] = skin;
                info.missing_texture = false;
            }
            let lit = matches!(lighting, Lighting::Lit { .. } | Lighting::VertexColours);
            let material = shader_material(shader, lit, bound, cutout, Kind::ObjectPart, &mut info);
            let draw = Draw { mesh, world, bones: matrices, projection: overlay.map(|o| o.0), viewport: overlay.map(|o| o.1), view: Some(view), material, lighting: lighting.clone(), id: 0 };
            out.push(Made { draw, from: 0, info });
        }
    }
}

/// The four bones a blended batch skins with: the bone of each of its matrices (by their index
/// in the batch, up to four), a matrix with none (or an index out of range) taking the first's.
fn skin_matrices(batch: &housefire::model::SubModel, bone: impl Fn(u16) -> Option<Mat3x4>) -> [Mat3x4; 4] {
    let mut four = [IDENTITY; 4];
    let mut set = [false; 4];
    for &(slot, index) in &batch.matrices {
        if let (Some(b), Some(s)) = (bone(slot), four.get_mut(usize::from(index))) {
            *s = b;
            set[usize::from(index)] = true;
        }
    }
    let first = four[0];
    for (k, s) in set.iter().enumerate() {
        if !s {
            four[k] = first;
        }
    }
    four
}

/// A study, not used to draw: the headings (eighths of a turn) that put each Sim's bones where
/// the log's '$' records have them, for each of `figures` the eighth whose bones come closest to
/// the logged matrices of `tick`. The engine's own heading (`Replay::sim_faces`) equals them for
/// both Sims at ticks 1,039 and 1,040.
pub fn headings_from_log(log: &housefire::tree_log::Log, tick: u32, wardrobe: &Wardrobe, figures: &[Figure]) -> HashMap<i16, u8> {
    let mut logged: Vec<[f32; 12]> = Vec::new();
    for k in log.skins.iter().filter(|k| k.tick == tick) {
        for m in k.bones {
            if !logged.contains(&m) {
                logged.push(m);
            }
        }
    }
    let mut out = HashMap::new();
    for f in figures {
        let score = |h: u8| -> f32 {
            let rows = f.bone_rows(wardrobe, h);
            logged.iter().map(|l| rows.iter().map(|r| (0..12).map(|i| (r[i] - l[i]).abs()).fold(0.0f32, f32::max)).fold(f32::MAX, f32::min).min(1.0)).sum()
        };
        if let Some(best) = (0..8u8).min_by(|&a, &b| score(a).total_cmp(&score(b))) {
            out.insert(f.id, best);
        }
    }
    out
}

/// The world's Sims on the lot as figures: their descriptions from the cast, their places, their
/// plays (`Replay::sim_plays`) and headings (the way each turned at its last walk's end,
/// `Replay::sim_faces`, else the facing the lot gave it).
pub fn figures_of(world: &housefire::world::World, cast: &Cast, plays: &[(i16, u32, f32, bool)], faces: &[(i16, (i32, i32))]) -> Vec<Figure> {
    world
        .objects()
        .filter(|o| o.person && o.on_lot())
        .filter_map(|o| {
            let description = *cast.description(o.person_data.get(31).copied()?)?;
            let play = plays.iter().find(|p| p.0 == o.id()).map(|p| (p.1, p.2, p.3));
            let heading = faces.iter().find(|f| f.0 == o.id()).and_then(|f| figure::heading_along(f.1.0, f.1.1)).unwrap_or(o.facing().rem_euclid(8) as u8);
            let carrying = world.objects().any(|h| h.data.get(housefire::world::DATA_CONTAINER) == Some(&o.id()));
            let mood = o.motives.get(figure::FACE_MOTIVE).copied().unwrap_or(0.0);
            Some(Figure { id: o.id(), description, place: [o.x16 as f32 / 16.0, o.y16 as f32 / 16.0], heading, play, carrying, mood })
        })
        .collect()
}

/// The Sim whose portrait a log-mode frame shows: the one whose outfit holds the most of the
/// overlay draws' parts (the engine has no player: the caller names the Sim shown).
pub fn portrait_sim(scene: &Scene, w: &Wardrobe, figures: &[Figure]) -> Option<i16> {
    let shown: Vec<&str> = scene.frame.draws.iter().map(|d| &scene.drawn[d.id as usize - 1]).filter(|i| i.class == Class::Overlay).map(|i| &*i.model).collect();
    figures
        .iter()
        .max_by_key(|f| {
            let outfit = w.cast.tables.dress(&f.description);
            let names: Vec<&str> = Slot::ALL.iter().filter_map(|&s| w.models.parts.record_by_id(outfit.part(s))).map(|r| &*w.models.parts.record(r).name).collect();
            shown.iter().filter(|m| names.contains(m)).count()
        })
        .filter(|_| !shown.is_empty())
        .map(|f| f.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::scene::apply;

    #[test]
    fn the_portrait_looks_back_at_a_sim_from_in_front() {
        // the logged view for the Sim at (24.5, 19.0) facing -y (a turn of 4 eighths)
        let v = portrait_view([24.5, 19.0], 4);
        let logged = [0.9999997, 0.0007947092, -1.1850022e-7, -24.515118, 6.639949e-5, -0.083403304, 0.9965159, -0.11260597, 0.0007919304, -0.9965156, -0.08340333, 17.854856];
        for (a, b) in v.iter().zip(logged) {
            // (the pitch and yaw were refined against the picture: within 0.03 of the logged view)
            assert!((a - b).abs() < 0.03, "{v:?}");
        }
        // a point at the Sim's head height straight ahead of the camera is at its distance
        let p = apply(&v, [24.5, 19.0, 1.6]);
        assert!((p[2] + PORTRAIT_DISTANCE).abs() < 0.01, "{p:?}");
    }
}
