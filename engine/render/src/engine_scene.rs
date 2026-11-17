//! The lot's model draws made from the engine's own state instead of the log's records
//! (`docs/assets.md`, "The frame from the engine's state"): `housefire::drawlist` says which parts
//! are drawn at which world matrices; here each gets its material from its sub-model's shader
//! (`shaders.arc`), its textures from the archives by the layers' name hashes, its GX state from
//! the material and its light set from the engine's lights.

use crate::frame::*;
use crate::scene::{material_of, mesh_of, Assets, Class, DrawInfo, Scene};
use housefire::drawlist::{Draw as Planned, Kind, Models};
use housefire::shader::Shader;
use housefire::tree_log::MaterialRec;
use std::collections::HashMap;

/// A planned draw as the renderer takes it, with its account.
pub struct Made {
    pub draw: Draw,
    pub info: DrawInfo,
}

/// What a sub-model's shader says the device's material is: its layers as the device's
/// `MaterialRec` holds them (the flag word at +4, the seven bytes at +0x10), the factor byte
/// 0xFF a shader keeps as 0x80 when the game sets the material up (`docs/assets.md`,
/// "Shaders").
pub const RUNTIME_FACTOR: (u8, u8) = (0xFF, 0x80);

fn material_record(shader: &Shader) -> MaterialRec {
    let mut layer = [[0u8; 0x18]; 2];
    for (l, s) in layer.iter_mut().zip(&shader.layers) {
        l[4..8].copy_from_slice(&s.flags.to_be_bytes());
        l[0x10..0x17].copy_from_slice(&s.bytes);
        if l[0x14] == RUNTIME_FACTOR.0 {
            l[0x14] = RUNTIME_FACTOR.1;
        }
    }
    MaterialRec { tick: 0, layers: shader.layers.len().min(2) as u32, layer, textures: [(0, [0; 128]); 2], strips_before: 0 }
}

/// The sampler a texture's header flags (`LFXT` +12) ask for: its wrap, whether it filters and
/// how it reads its mip levels.
fn sampler_of(flags: u16, levels: u32) -> ((Wrap, Wrap), bool, Mipmaps) {
    let _ = flags;
    ((Wrap::Repeat, Wrap::Repeat), true, if levels > 1 { Mipmaps::Linear } else { Mipmaps::Off })
}

/// What the engine mode needs from the caller besides the draws.
pub struct Inputs<'a> {
    pub draws: &'a [Planned],
    pub models: &'a Models,
    pub shaders: &'a HashMap<u32, Shader>,
    /// The light set of a draw.
    pub lighting: &'a dyn Fn(&Planned) -> Lighting,
    /// The view the draws are seen through (the camera's).
    pub view: Mat3x4,
}

impl Assets {
    /// The renderer's draws for the engine's list. Meshes and textures are added to `scene`'s
    /// (the draws' ids are left for the caller, who places them).
    pub fn engine_draws(&self, scene: &mut Scene, input: &Inputs<'_>) -> Vec<Made> {
        let mut mesh_of_batch: HashMap<(usize, bool), Option<MeshId>> = HashMap::new();
        let mut textures: HashMap<u32, Option<(TextureRef, bool)>> = HashMap::new();
        let mut out = Vec::new();
        for d in input.draws {
            let Some(part) = input.models.parts.parts_of(d.part.0).find(|p| p.index == d.part) else { continue };
            let list = part.batch.list.is_some();
            let mesh = *mesh_of_batch.entry((part.batch as *const _ as usize, list)).or_insert_with(|| {
                mesh_of(part.batch, list).map(|m| {
                    scene.meshes.push(m);
                    MeshId(scene.meshes.len() as u32 - 1)
                })
            });
            let Some(mesh) = mesh else { continue };
            let shader = input.shaders.get(&d.shader.unwrap_or(part.batch.hash));
            let mut info = DrawInfo { class: if list { Class::DisplayList } else { Class::RigidStrip }, strip: 0, model: part.record.name.clone(), missing_texture: false, textures: [None, None], flags: [0; 2], unsupported: None, skin: None };
            let mut bound: [Option<TextureRef>; 2] = [None, None];
            let mut cutout = false;
            if let Some(sh) = shader {
                for (slot, l) in sh.layers.iter().take(2).enumerate() {
                    let found = textures.entry(l.texture).or_insert_with(|| {
                        let hash = *self.by_name.get(&l.texture)?;
                        let levels = self.decode(hash)?;
                        let (wrap, linear, mipmaps) = sampler_of(self.by_hash[&hash].flags, levels.len() as u32);
                        let cutout = levels[0].2.iter().any(|&p| p >> 24 != 255);
                        scene.textures.push(levels);
                        Some((TextureRef { texture: TextureId(scene.textures.len() as u32 - 1), wrap, linear, mipmaps }, cutout))
                    });
                    bound[slot] = found.map(|f| f.0);
                    if slot == 0 {
                        cutout = found.is_some_and(|f| f.1);
                    }
                    info.missing_texture |= found.is_none();
                    info.textures[slot] = self.by_name.get(&l.texture).and_then(|h| self.by_hash.get(h)).map(|s| s.name.clone());
                }
            }
            let lighting = (input.lighting)(d);
            let lit = matches!(lighting, Lighting::Lit { .. } | Lighting::VertexColours);
            let (mut material, unsupported) = match shader {
                Some(sh) => material_of(&material_record(sh), lit, bound),
                None => material_of(&MaterialRec { tick: 0, layers: 0, layer: [[0; 0x18]; 2], textures: [(0, [0; 128]); 2], strips_before: 0 }, lit, bound),
            };
            info.unsupported = unsupported;
            state_of(&mut material, d.kind, cutout);
            out.push(Made {
                draw: Draw { mesh, world: d.world, bones: None, projection: None, viewport: None, view: Some(input.view), material, lighting, id: 0 },
                info,
            });
        }
        out
    }
}

/// The alpha (of 255) an opaque material's draw tests its texture's alpha against, and the
/// blended ones': what the records show (`docs/assets.md`, "The frame from the engine's state").
pub const ALPHA_TEST_OPAQUE: u8 = 127;
pub const ALPHA_TEST_BLENDED: u8 = 5;
/// The destination alpha a draw writes (GXSetDstAlpha: what the shadows' blend reads back): 0 for
/// the objects, which the shadow passes leave as they are, the ground's for the lot's own model.
pub const DST_ALPHA_OBJECT: u8 = 0;
pub const DST_ALPHA_GROUND: u8 = 0x7F;

/// The GX state of a draw as the records show it for its kind (`docs/assets.md`, "The renderer":
/// Z less-or-equal with writes, no culling; a one-layer opaque material whose texture has clear
/// pixels tests alpha at least 127, the blended ones at least 5, with source alpha over the
/// picture).
fn state_of(m: &mut Material, kind: Kind, cutout: bool) {
    m.depth_test = Some(Compare::LEQUAL);
    m.depth_write = true;
    m.cull = Cull::None;
    m.alpha_test = match m.blend {
        Blend::Opaque => (cutout && m.layers[1].is_none()).then(|| AlphaTest::at_least(ALPHA_TEST_OPAQUE)),
        _ => Some(AlphaTest::at_least(ALPHA_TEST_BLENDED)),
    };
    m.dst_alpha = Some(if kind == Kind::LotList { DST_ALPHA_GROUND } else { DST_ALPHA_OBJECT });
}

/// What the engine's own state gives for a frame: the lot's model draws.
pub struct Plan {
    pub models: Models,
    pub draws: Vec<Planned>,
    pub placements: Vec<housefire::scene::Placement>,
    /// The GUIDs of each placement's parts, by leader.
    pub guids: std::collections::BTreeMap<i16, Vec<u32>>,
}

/// The engine's lot (the disc's, or the card's save: `card` the memory card image; the lot the
/// log ran on, `housefire::lotpick`) and its draws for `tick`, seen from the eye of `view`. The
/// loaded lot is the world as the game starts the lot; the animation clock is the tick's.
pub fn plan(disc: &housefire::disc::Disc, card: Option<&std::path::Path>, log: &housefire::tree_log::Log, tick: u32, view: &Mat3x4, house: usize) -> Option<Plan> {
    use housefire::{library::Library, save, world::World};
    let ngh = match card {
        Some(p) => {
            let mut bytes = std::fs::read(p).ok()?;
            bytes.truncate(1 << 22);
            save::memory_card_file(&bytes, "G4ME69").ok()?
        }
        None => disc.read("Runtime/default.ngh")?,
    };
    let lib = Library::bustin_out(disc);
    let (lots, _, _, best, _) = housefire::lotpick::pick_lot(&lib, &ngh, log);
    let mut world = World::new(&lib);
    world.load_lot(&lots[best]).ok()?;
    let quick = disc.read("quickdat.arc")?;
    let res = housefire::arc::entries(&quick)?.into_iter().find(|e| e.name == "SimsObjects").and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e))).map(|img| housefire::quickdata::object_resources(&img))?;
    let models = Models::read(disc);
    // the eye: -R^T t of the view
    let eye = [0, 1].map(|i| -(0..3).map(|r| view[4 * r + i] * view[4 * r + 3]).sum::<f32>());
    let lot = housefire::drawlist::lot_draws_of(&world, &res, &models, disc, house, tick, Some(eye));
    if std::env::var_os("HF_OBJECTS").is_some() {
        let list = log.lists.iter().rfind(|l| l.tick <= tick);
        for o in world.objects().filter(|o| o.on_lot() && !o.person) {
            println!("  object {} guid {:08x} at ({}, {}) level {} leader {} flags {:?} in the log's list {:?}", o.id(), o.guid, o.x16, o.y16, o.level, o.leader, o.data.get(housefire::world::DATA_FLAGS), list.map(|l| l.ids.contains(&o.id())));
        }
    }
    let guids = lot.placements.iter().map(|p| (p.leader, world.find_object(p.leader).into_iter().chain(world.objects().filter(|o| o.leader == p.leader)).map(|o| o.guid).collect())).collect();
    Some(Plan { models, draws: lot.draws, placements: lot.placements, guids })
}

/// Puts the engine's draws where the log's first rigid or display-list draw was and takes the log's
/// out (those of the Sims and what they hold stay the log's: they are the Sims' own, counted with
/// the skinned parts). The opaque draws come first, then the blended ones. Returns how many log
/// draws it took out.
pub fn splice(scene: &mut Scene, made: Vec<Made>) -> usize {
    let is_model = |d: &Draw, drawn: &[DrawInfo]| matches!(drawn[d.id as usize - 1].class, Class::RigidStrip | Class::DisplayList);
    for d in &scene.frame.draws {
        let info = &mut scene.drawn[d.id as usize - 1];
        if matches!(info.class, Class::RigidStrip | Class::DisplayList) && (housefire::drawlist::is_sims(&info.model) || housefire::drawlist::is_held(&info.model)) {
            info.class = Class::Skinned;
        }
    }
    let (mut opaque, mut blended): (Vec<Made>, Vec<Made>) = made.into_iter().partition(|m| m.draw.material.blend == Blend::Opaque);
    opaque.append(&mut blended);
    let mut engine = Some(opaque);
    let mut out = Vec::new();
    let mut removed = 0;
    for d in std::mem::take(&mut scene.frame.draws) {
        if !is_model(&d, &scene.drawn) {
            out.push(d);
            continue;
        }
        removed += 1;
        if let Some(list) = engine.take() {
            for mut m in list {
                scene.drawn.push(m.info.clone());
                scene.all.push(m.info);
                m.draw.id = scene.drawn.len() as u32;
                out.push(m.draw);
            }
        }
    }
    scene.frame.draws = out;
    removed
}
