//! The Sims drawn from the engine's own state (`docs/assets.md`, "The Sims from the engine's
//! state"): each Sim's parts from its description and the part tables, its bones from its
//! animation (`housefire::figure`), the skinned parts blended on the GPU through them, the
//! shadows, and the portrait of one Sim in the corner.
//!
//! What still comes from the log: the Sims' run-time textures (the one 128 x 128 image the
//! game makes of a Sim's skin, face, hair and clothes: `SimSkins`) and, where the engine does
//! not know it, a Sim's heading.

use crate::engine_scene::{lighting_for, material_record, shadow_material, state_of, Caches, Made};
use crate::frame::*;
use crate::scene::{material_of, mesh_of, Assets, Class, DrawInfo, Scene, PORTRAIT_BACKDROP, PORTRAIT_BACKDROP_COLOUR};
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
/// in front and 1.6014 up; pitch 0.0835 rad, yaw 0.0008), the pitch and yaw then refined
/// against the portrait's own pixels (a coordinate descent on the count of Sims' pixels beyond
/// 4: 412 to 207; the logged ones are the camera of the original's own head, which the engine's
/// pose does not reach to within 0.01, so the picture wants a slightly lower look).
pub const PORTRAIT_DISTANCE: f32 = 1.1973;
pub const PORTRAIT_HEIGHT: f32 = 1.6014;
pub const PORTRAIT_PITCH: f32 = 0.0825;
pub const PORTRAIT_YAW: f32 = -0.00015;

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

/// The Sims' run-time textures, as the log has them: the image of each outfit part name the log
/// drew with one, its levels and how the draw sampled it.
#[derive(Default)]
pub struct SimSkins {
    by_part: HashMap<String, Skin>,
}

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
                by_part.entry(info.model.clone()).or_insert_with(|| (scene.textures[t.texture.0 as usize].clone(), t));
            }
        }
        SimSkins { by_part }
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
    pub shaders: &'a HashMap<u32, Shader>,
}

/// Per-run caches of the Sims' meshes and textures.
#[derive(Default)]
pub struct SimCaches {
    meshes: HashMap<usize, Option<MeshId>>,
    skins: HashMap<String, TextureRef>,
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

impl Assets {
    /// The Sims' draws for `figures`, standing in the lot lit by `lights`, seen through `view`
    /// (`sun`: the direction the sun shines in, for the shadows outdoors); the portrait of
    /// `portrait` (a Sim's id) when it is on the lot.
    #[allow(clippy::too_many_arguments)]
    pub fn engine_sims(&self, scene: &mut Scene, caches: &mut Caches, cache: &mut SimCaches, w: &Wardrobe, figures: &[Figure], lights: &LotLights, sun: [f32; 3], view: Mat3x4, portrait: Option<i16>) -> SimDraws {
        let mut out = SimDraws::default();
        for f in figures {
            let nodes = w.rigs.of(&f.description);
            let pose = f.play.and_then(|(id, at, backward)| w.catalog.by_id.get(&id).map(|a| (a, figure::pose_place(a.frames, at, backward))));
            let bones: Vec<Mat3x4> = figure::bones(nodes, pose, &figure::root(f.place[0], f.place[1], f.heading)).iter().map(figure::rows).collect();
            let outfit = w.cast.tables.dress(&f.description);
            let records: Vec<(Slot, usize)> = Slot::ALL.iter().filter_map(|&s| Some((s, w.models.parts.record_by_id(outfit.part(s))?))).collect();
            let skin_names: Vec<String> = HOUSE_ORDER.iter().filter_map(|s| records.iter().find(|r| r.0 == *s)).map(|r| w.models.parts.record(r.1).name.clone()).collect();
            let skin = skin_names.iter().find(|n| w.skins.by_part.contains_key(*n)).map(|n| {
                let (levels, tex) = &w.skins.by_part[n];
                *cache.skins.entry(n.clone()).or_insert_with(|| {
                    scene.textures.push(levels.clone());
                    TextureRef { texture: TextureId(scene.textures.len() as u32 - 1), ..*tex }
                })
            });
            let room = lights.room_of_place(f.place[0], f.place[1]);
            // a Sim takes the four lights of the light grid's point of the tile it stands in (an
            // object's are the nearest whole tile's: the logged sets of both Sims of frame 2400 are
            // the tile's, which a position on a tile's half (24.5) tells apart)
            let grid = [f.place[0].floor(), f.place[1].floor()];
            let house_light = {
                let Lighting::Lit { ambient, mut directionals, points } = lighting_for(&lights.at_in(f.place, grid), &view) else { unreachable!() };
                let to_light = std::array::from_fn(|r| view[r * 4] * SIM_FILL.1[0] + view[r * 4 + 1] * SIM_FILL.1[1] + view[r * 4 + 2] * SIM_FILL.1[2]);
                directionals.push(Directional { colour: [SIM_FILL.0; 3], to_light });
                Lighting::Lit { ambient, directionals, points }
            };
            let order = |slots: &[Slot]| -> Vec<usize> { slots.iter().filter_map(|s| records.iter().find(|r| r.0 == *s).map(|r| r.1)).collect() };
            for r in order(&HOUSE_ORDER) {
                self.sim_part_draws(scene, caches, cache, w, r, &bones, skin, &house_light, view, None, &mut out.house);
            }
            if portrait == Some(f.id) {
                let pview = portrait_view(f.place, f.heading);
                let plight = Lighting::Lit {
                    ambient: PORTRAIT_AMBIENT,
                    directionals: PORTRAIT_LIGHTS.iter().map(|&(colour, to_light)| Directional { colour, to_light }).collect(),
                    points: Vec::new(),
                };
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
                let sview = if room == 0 { crate::engine_scene::shadow_view(&view, sun, crate::engine_scene::SHADOW_SHEAR_OBJECT) } else { crate::engine_scene::shadow_view(&view, [0.0; 3], 1.0) };
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

    /// The draws of every batch of record `r` through the Sim's `bones`: a batch on one bone
    /// hangs from it (its matrix is the draw's world), a blended one takes its matrices' bones
    /// (up to four) for the GPU's skinning.
    #[allow(clippy::too_many_arguments)]
    fn sim_part_draws(&self, scene: &mut Scene, caches: &mut Caches, cache: &mut SimCaches, w: &Wardrobe, r: usize, bones: &[Mat3x4], skin: Option<TextureRef>, lighting: &Lighting, view: Mat3x4, overlay: Option<([f32; 6], Viewport)>, out: &mut Vec<Made>) {
        for part in w.models.parts.parts_of(r) {
            let batch = part.batch;
            let bone = |slot: u16| bones.get(usize::from(slot)).copied();
            let (mesh, world, matrices) = if part.skinned() {
                let mesh = *cache.meshes.entry(batch as *const _ as usize).or_insert_with(|| {
                    skinned_mesh_of(batch).map(|m| {
                        scene.meshes.push(m);
                        MeshId(scene.meshes.len() as u32 - 1)
                    })
                });
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
                (mesh, IDENTITY, Some(Box::new(four)))
            } else {
                let Some(&(slot, _)) = batch.matrices.first() else { continue };
                let Some(world) = bone(slot) else { continue };
                let list = batch.list.is_some();
                let mesh = *caches.meshes.entry((batch as *const _ as usize, list)).or_insert_with(|| {
                    mesh_of(batch, list).map(|m| {
                        scene.meshes.push(m);
                        MeshId(scene.meshes.len() as u32 - 1)
                    })
                });
                (mesh, world, None)
            };
            let Some(mesh) = mesh else { continue };
            let shader = w.shaders.get(&batch.hash);
            let class = if overlay.is_some() { Class::Overlay } else { Class::Skinned };
            let mut info = DrawInfo { class, strip: 0, model: part.record.name.clone(), missing_texture: false, textures: [None, None], flags: [0; 2], unsupported: None, skin: None };
            let (mut bound, cutout) = self.bind_shader(scene, &mut caches.textures, shader, &mut info);
            // the shader's blank layer names the Sim's own image
            if shader.is_some_and(|sh| sh.name == SIM_BLANK_SHADER) {
                bound[0] = skin;
                info.missing_texture = false;
            }
            let lit = matches!(lighting, Lighting::Lit { .. } | Lighting::VertexColours);
            let (mut material, unsupported) = match shader {
                Some(sh) => material_of(&material_record(sh), lit, bound),
                None => material_of(&housefire::tree_log::MaterialRec { tick: 0, layers: 0, layer: [[0; 0x18]; 2], textures: [(0, [0; 128]); 2], strips_before: 0 }, lit, bound),
            };
            info.unsupported = unsupported;
            state_of(&mut material, Kind::ObjectPart, cutout, shader.and_then(|sh| sh.layers.first()).map(|l| l.value));
            let draw = Draw { mesh, world, bones: matrices, projection: overlay.map(|o| o.0), viewport: overlay.map(|o| o.1), view: Some(view), material, lighting: lighting.clone(), id: 0 };
            out.push(Made { draw, from: 0, info });
        }
    }
}

/// The headings (eighths of a turn) that put each Sim's bones where the log's '$' records have
/// them: the engine does not keep a Sim's heading apart from its `facing` field, which is the
/// way it was last placed, not the way it stands after it turned (the cook at the stove). For
/// each of `figures`, the eighth whose bones come closest to the logged matrices of `tick`.
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
        let nodes = wardrobe.rigs.of(&f.description);
        let pose = f.play.and_then(|(id, at, backward)| wardrobe.catalog.by_id.get(&id).map(|a| (a, figure::pose_place(a.frames, at, backward))));
        let score = |h: u8| -> f32 {
            let rows: Vec<Mat3x4> = figure::bones(nodes, pose, &figure::root(f.place[0], f.place[1], h)).iter().map(figure::rows).collect();
            logged.iter().map(|l| rows.iter().map(|r| (0..12).map(|i| (r[i] - l[i]).abs()).fold(0.0f32, f32::max)).fold(f32::MAX, f32::min).min(1.0)).sum()
        };
        if let Some(best) = (0..8u8).min_by(|&a, &b| score(a).total_cmp(&score(b))) {
            out.insert(f.id, best);
        }
    }
    out
}

/// The world's Sims on the lot as figures: their descriptions from the cast, their places, their
/// plays (`Replay::sim_plays`) and headings (`headings`, else their facing).
pub fn figures_of(world: &housefire::world::World, cast: &Cast, plays: &[(i16, u32, f32, bool)], headings: &HashMap<i16, u8>) -> Vec<Figure> {
    world
        .objects()
        .filter(|o| o.person && o.on_lot())
        .filter_map(|o| {
            let description = *cast.description(o.person_data.get(31).copied()?)?;
            let play = plays.iter().find(|p| p.0 == o.id()).map(|p| (p.1, p.2, p.3));
            // (a position is in sixteenths of a tile; the facing's eighths turn the other way round in the root)
            let heading = headings.get(&o.id()).copied().unwrap_or((o.facing().rem_euclid(8)) as u8);
            Some(Figure { id: o.id(), description, place: [o.x16 as f32 / 16.0, o.y16 as f32 / 16.0], heading, play })
        })
        .collect()
}

/// The Sim whose portrait a log-mode frame shows: the one whose outfit holds the most of the
/// overlay draws' parts (the engine has no player: the caller names the Sim shown).
pub fn portrait_sim(scene: &Scene, w: &Wardrobe, figures: &[Figure]) -> Option<i16> {
    let shown: Vec<&str> = scene.frame.draws.iter().map(|d| &scene.drawn[d.id as usize - 1]).filter(|i| i.class == Class::Overlay).map(|i| i.model.as_str()).collect();
    figures
        .iter()
        .max_by_key(|f| {
            let outfit = w.cast.tables.dress(&f.description);
            let names: Vec<&str> = Slot::ALL.iter().filter_map(|&s| w.models.parts.record_by_id(outfit.part(s))).map(|r| w.models.parts.record(r).name.as_str()).collect();
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
