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
    /// Its place in the planned list (`Inputs::draws`).
    pub from: usize,
    pub info: DrawInfo,
}

/// What a sub-model's shader says the device's material is: its layers as the device's
/// `MaterialRec` holds them (the flag word at +4, the seven bytes at +0x10), the factor byte
/// 0xFF a shader keeps as 0x80 when the game sets the material up (`docs/assets.md`,
/// "Shaders"), except in a sphere-mapped layer (texgen 2: the reflecting doors, windows, mirrors),
/// where the log's draws keep the full 1.0.
pub const RUNTIME_FACTOR: (u8, u8) = (0xFF, 0x80);

/// The layer's texgen byte (+0x16) of a sphere map.
const SPHERE_TEXGEN: u8 = 2;

pub(crate) fn material_record(shader: &Shader) -> MaterialRec {
    let mut layer = [[0u8; 0x18]; 2];
    for (l, s) in layer.iter_mut().zip(&shader.layers) {
        l[4..8].copy_from_slice(&s.flags.to_be_bytes());
        l[0x10..0x17].copy_from_slice(&s.bytes);
        if l[0x14] == RUNTIME_FACTOR.0 && l[0x16] != SPHERE_TEXGEN {
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
    pub lighting: &'a dyn Fn(&Planned, MaterialFactors) -> Lighting,
    /// The view the draws are seen through (the camera's).
    pub view: Mat3x4,
}

/// The junk texture of the wall models' 7-vertex pieces (a wall's foot, a door's or window's): the
/// original draws no part that names it (none of the log's draws does; the porch floor shows at
/// the walls' feet where the engine had drawn them black).
pub const PLACEHOLDER_TEXTURE: &str = "_garbage";

/// The textures bound so far, by the layer's name hash: the texture and whether it has clear pixels.
pub type TextureCache = HashMap<u32, Option<(TextureRef, bool)>>;

/// What the draws made so far added to a scene, to be found again instead of added again: a
/// window that draws every tick keeps one for all its frames, so that the meshes and textures go
/// to the GPU once.
#[derive(Default)]
pub struct Caches {
    /// A model's batch (its address in the `Models`) as a mesh, as a display list or as strips.
    meshes: HashMap<(usize, bool), Option<MeshId>>,
    pub(crate) textures: TextureCache,
    /// A floor strip's mesh by a hash of its corners.
    floors: HashMap<u64, MeshId>,
    /// Whether the floors' meshes are the frame's own (`Scene::transient`, ids `TRANSIENT_MESH`
    /// and the place) instead of added to the scene's for good: for a window, where the light
    /// moves their colours every tick.
    pub transient_floors: bool,
}

impl Caches {
    /// The mesh of a model's batch, as a display list or as strips, added to `scene` the first time.
    pub(crate) fn mesh(&mut self, scene: &mut Scene, batch: &housefire::model::SubModel, list: bool) -> Option<MeshId> {
        *self.meshes.entry((batch as *const _ as usize, list)).or_insert_with(|| mesh_of(batch, list).map(|m| scene.add_mesh(m)))
    }
}

/// The material record of a draw with no shader of its own: `layers` layers, all zero.
pub(crate) fn blank_record(layers: u32) -> MaterialRec {
    MaterialRec { tick: 0, layers, layer: [[0; 0x18]; 2], textures: [(0, [0; 128]); 2], strips_before: 0 }
}

/// A draw's material from its shader (none: the blank one) with the textures bound to it, in the
/// GX state of its `kind`; `info` takes what the material lacks.
pub(crate) fn shader_material(shader: Option<&Shader>, lit: bool, bound: [Option<TextureRef>; 2], cutout: bool, kind: Kind, info: &mut DrawInfo) -> Material {
    let (mut material, unsupported) = match shader {
        Some(sh) => material_of(&material_record(sh), lit, bound),
        None => material_of(&blank_record(0), lit, bound),
    };
    info.unsupported = unsupported;
    state_of(&mut material, kind, cutout, shader.and_then(|sh| sh.layers.first()).map(|l| l.value));
    material
}

impl Assets {
    /// The textures of a shader's first two layers (their names by hash in the archives), added to
    /// `scene`'s when not in `cache` yet, and whether the first one has clear pixels; `info` takes
    /// their names and whether one was not found.
    pub fn bind_shader(&self, scene: &mut Scene, cache: &mut TextureCache, shader: Option<&Shader>, info: &mut DrawInfo) -> ([Option<TextureRef>; 2], bool) {
        let mut bound: [Option<TextureRef>; 2] = [None, None];
        let mut cutout = false;
        if let Some(sh) = shader {
            for (slot, l) in sh.layers.iter().take(2).enumerate() {
                let found = cache.entry(l.texture).or_insert_with(|| {
                    let hash = *self.by_name.get(&l.texture)?;
                    let levels = self.decode(hash)?;
                    let (wrap, linear, mipmaps) = sampler_of(self.by_hash[&hash].flags, levels.len() as u32);
                    let cutout = levels[0].2.iter().any(|&p| p >> 24 != 255);
                    Some((TextureRef { texture: scene.add_texture(levels), wrap, linear, mipmaps }, cutout))
                });
                bound[slot] = found.map(|f| f.0);
                if slot == 0 {
                    cutout = found.is_some_and(|f| f.1);
                }
                info.missing_texture |= found.is_none();
                info.textures[slot] = self.by_name.get(&l.texture).and_then(|h| self.by_hash.get(h)).map(|s| s.name.clone());
            }
        }
        (bound, cutout)
    }

    /// The renderer's draws for the engine's list. Meshes and textures are added to `scene`'s
    /// (the draws' ids are left for the caller, who places them).
    pub fn engine_draws(&self, scene: &mut Scene, caches: &mut Caches, input: &Inputs<'_>) -> Vec<Made> {
        let mut out = Vec::new();
        for (from, d) in input.draws.iter().enumerate() {
            let Some(part) = input.models.parts.parts_of(d.part.0).find(|p| p.index == d.part) else { continue };
            let list = part.batch.list.is_some();
            let Some(mesh) = caches.mesh(scene, part.batch, list) else { continue };
            let shader = input.shaders.get(&d.shader.unwrap_or(part.batch.hash));
            let mut info = DrawInfo::new(if list { Class::DisplayList } else { Class::RigidStrip }, 0, part.record.name.clone());
            let (bound, cutout) = self.bind_shader(scene, &mut caches.textures, shader, &mut info);
            if info.textures[0].as_deref() == Some(PLACEHOLDER_TEXTURE) {
                continue;
            }
            let lighting = (input.lighting)(d, shader.and_then(|sh| sh.material()));
            let lit = matches!(lighting, Lighting::Lit { .. } | Lighting::VertexColours);
            let material = shader_material(shader, lit, bound, cutout, d.kind, &mut info);
            out.push(Made {
                draw: Draw { mesh, world: d.world, bones: None, projection: None, viewport: None, view: Some(input.view), material, lighting, id: 0 },
                from,
                info,
            });
        }
        out
    }
}

/// How far a shadow falls: the shadow draws' view is the camera's with the lot's z sheared onto the
/// ground, a point at height z moved by z times the sun's x and y over a divisor. The logged views
/// of frame 2400 (sun (0.8223, -0.2109, -0.5285)) shear by (0.7188, -0.1843) the outdoor objects'
/// draws (a divisor of 1.144) and by (0.8855, -0.2271) the wall pieces' and fences' (0.9286), none
/// of them the sun's own vertical part. Fitted at this one clock. The indoor objects' shears
/// (each a different small vector, from their light sets) are not found: they are not sheared.
pub const SHADOW_SHEAR_OBJECT: f32 = 1.144;
pub const SHADOW_SHEAR_WALL: f32 = 0.9286;

/// The view the shadows are drawn through: the camera's `view` after the shear that puts a point
/// of height z on the ground along the sun's direction `sun`, over `divisor`.
pub fn shadow_view(view: &Mat3x4, sun: [f32; 3], divisor: f32) -> Mat3x4 {
    let (a, b) = (sun[0] / divisor, sun[1] / divisor);
    let mut v = *view;
    for r in 0..3 {
        v[r * 4 + 2] = a * view[r * 4] + b * view[r * 4 + 1];
    }
    v
}

/// The material of a projected shadow's draw, as the log's 692 shadow draws of frame 2400 all
/// have it: flat black (lighting off), blended by the picture's destination alpha (the floor's
/// 0x7F, which a shadow leaves 0 so overlapping shadows do not stack), Z off, alpha test at least
/// 5, a constant destination alpha of 0. The polygon texture's alpha is not applied (`scene.rs`,
/// `shadow_of`).
pub fn shadow_material() -> Material {
    Material {
        combine: Combine::Vertex,
        layers: [None, None],
        blend: Blend::Mix { src: Factor::DstAlpha, dst: Factor::InvDstAlpha },
        cull: Cull::None,
        depth_test: None,
        depth_write: false,
        alpha_test: Some(AlphaTest::at_least(ALPHA_TEST_BLENDED)),
        write_colour: true,
        write_alpha: true,
        dst_alpha: Some(0),
        layer1_from_position: None,
        sphere: None,
    }
}

impl Assets {
    /// The shadow pass's draws of the planned parts (`drawlist::LotDraws::shadows`): each part's
    /// vertex strip (its display list is the lit pass's) in the shadow material and lighting off.
    pub fn engine_shadows(&self, scene: &mut Scene, caches: &mut Caches, models: &Models, shadows: &[Planned], view_of: &dyn Fn(&Planned) -> Mat3x4) -> Vec<(usize, Made)> {
        let mut out = Vec::new();
        for (k, d) in shadows.iter().enumerate() {
            let Some(part) = models.parts.parts_of(d.part.0).find(|p| p.index == d.part) else { continue };
            let Some(mesh) = caches.mesh(scene, part.batch, false) else { continue };
            let info = DrawInfo::new(Class::Shadow, 0, part.record.name.clone());
            out.push((k, Made { from: k, draw: Draw { mesh, world: d.world, bones: None, projection: None, viewport: None, view: Some(view_of(d)), material: shadow_material(), lighting: Lighting::Black, id: 0 }, info }));
        }
        out
    }
}

impl Plan {
    /// The room a planned shadow falls in: the outdoors (0) for a wall piece's, else its place's.
    fn shadow_room(&self, d: &Planned) -> usize {
        if d.kind == Kind::WallPiece { 0 } else { self.lights.room_of_place(d.origin[0], d.origin[1]) }
    }
}

impl Assets {
    /// The shadow pass's draws of the plan by the room they fall in, seen through `view` and
    /// `projection` with the sun shining in direction `sun`: a wall piece's shadow is sheared by
    /// the wall's divisor along the sun, an outdoor object's by the object's, an indoor one's not.
    pub fn engine_shadows_by_room(&self, scene: &mut Scene, caches: &mut Caches, plan: &Plan, view: &Mat3x4, projection: &[f32; 6], sun: [f32; 3]) -> Vec<Vec<Made>> {
        let view_of = |d: &Planned| match (d.kind, plan.shadow_room(d)) {
            (Kind::WallPiece, _) => shadow_view(view, sun, SHADOW_SHEAR_WALL),
            (_, 0) => shadow_view(view, sun, SHADOW_SHEAR_OBJECT),
            _ => shadow_view(view, [0.0; 3], 1.0),
        };
        let mut by_room: Vec<Vec<Made>> = Vec::new();
        for (k, m) in self.engine_shadows(scene, caches, &plan.models, &plan.shadows, &view_of) {
            let d = &plan.shadows[k];
            // (the original draws what its bounds put on the screen: the pieces' are not drawn off it)
            if d.kind == Kind::WallPiece && !housefire::drawlist::in_view(view, projection, &d.world) {
                continue;
            }
            let room = plan.shadow_room(d);
            if by_room.len() <= room {
                by_room.resize_with(room + 1, Vec::new);
            }
            by_room[room].push(m);
        }
        by_room
    }
}

/// A floor strip's draw: its corners' colours as the mesh's vertex colours, the shader's material
/// (the terrain's two layers cross-faded, an indoor floor's reflection), Z less-or-equal with
/// writes, a constant destination alpha of 0x7F (the stencil the shadows read, `shadow_material`).
/// The terrain's second layer is read at the position (`scene::shader_position_uv`).
pub const DST_ALPHA_FLOOR: u8 = 0x7F;

/// The mask pass over a floor strip, drawn after a batch's shadows: no colour, the alpha written 0
/// where the floor's texture is not clear (the shadows' stencil is cleared for the next batch),
/// no depth test; the floor's texture clamped and without mip levels, as the records show.
fn mask_of(floor: &Material) -> Material {
    let mut m = *floor;
    m.layers = [floor.layers[0].map(|l| TextureRef { wrap: (Wrap::Clamp, Wrap::Clamp), mipmaps: Mipmaps::Off, ..l }), None];
    m.combine = Combine::Modulate { unlit: false };
    m.blend = Blend::Mix { src: Factor::Other, dst: Factor::Zero };
    m.depth_test = None;
    m.depth_write = false;
    m.alpha_test = Some(AlphaTest::at_least(ALPHA_TEST_BLENDED));
    m.write_colour = false;
    m.write_alpha = true;
    m.dst_alpha = Some(0);
    m.layer1_from_position = None;
    m.sphere = None;
    m
}

/// The floor strips' draws and their masks, by strip (`housefire::floors::Strip` order).
pub struct FloorDraws {
    pub floor: Made,
    pub mask: Made,
}

impl Assets {
    pub fn engine_floors(&self, scene: &mut Scene, caches: &mut Caches, strips: &[housefire::floors::Strip], shaders: &HashMap<u32, Shader>, view: Mat3x4) -> Vec<FloorDraws> {
        let mut out = Vec::new();
        for (k, st) in strips.iter().enumerate() {
            let shader = shaders.get(&st.shader);
            let mut info = DrawInfo::new(Class::Floor, 0, format!("run{k}"));
            let (bound, _) = self.bind_shader(scene, &mut caches.textures, shader, &mut info);
            let record = match shader {
                Some(sh) => material_record(sh),
                None => blank_record(1),
            };
            let (mut material, unsupported) = material_of(&record, true, bound);
            info.unsupported = unsupported;
            if material.layers[1].is_some() {
                material.layer1_from_position = shader.and_then(|sh| sh.position_uv()).map(|(o, s)| crate::scene::shader_position_uv(o, s));
            }
            material.depth_test = Some(Compare::LEQUAL);
            material.depth_write = true;
            material.cull = Cull::None;
            material.dst_alpha = Some(DST_ALPHA_FLOOR);
            // (the logged floor draws test no alpha: the shader's flag would give the wood a test)
            material.alpha_test = None;
            let vertices: Vec<_> = st.corners.iter().map(|&(p, uv, c)| ([p[0], p[1], p[2], 1.0], uv, u32::from_be_bytes(c))).collect();
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                for (p, uv, c) in &st.corners {
                    (p.map(f32::to_bits), uv.map(f32::to_bits), c).hash(&mut h);
                }
                h.finish()
            };
            let mesh = if caches.transient_floors {
                scene.transient.push(crate::scene::floor_mesh(&vertices));
                MeshId(TRANSIENT_MESH | (scene.transient.len() as u32 - 1))
            } else {
                *caches.floors.entry(key).or_insert_with(|| {
                    scene.meshes.push(crate::scene::floor_mesh(&vertices));
                    MeshId(scene.meshes.len() as u32 - 1)
                })
            };
            let world = crate::scene::floor_world();
            let draw = |material: Material| Draw { mesh, world, bones: None, projection: None, viewport: None, view: Some(view), material, lighting: Lighting::VertexColours, id: 0 };
            let mask_info = DrawInfo { class: Class::FloorMask, ..info.clone() };
            out.push(FloorDraws { floor: Made { draw: draw(material), from: k, info }, mask: Made { draw: draw(mask_of(&material)), from: k, info: mask_info } });
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
pub(crate) fn state_of(m: &mut Material, kind: Kind, cutout: bool, reference: Option<f32>) {
    // the alpha test's reference is the shader's first layer's float (0.5 and 0.4: 127 and 102 of
    // 255, the logged ones; 0.02: 5), else the constants
    let at = |fallback: u8| reference.filter(|v| (0.0..=1.0).contains(v)).map_or(fallback, |v| (v * 255.0) as u8);
    m.depth_test = Some(Compare::LEQUAL);
    m.depth_write = true;
    m.cull = Cull::None;
    m.alpha_test = match m.blend {
        Blend::Opaque => (cutout && m.layers[1].is_none()).then(|| AlphaTest::at_least(at(ALPHA_TEST_OPAQUE))),
        _ => Some(AlphaTest::at_least(at(ALPHA_TEST_BLENDED))),
    };
    m.dst_alpha = Some(if kind == Kind::LotList { DST_ALPHA_GROUND } else { DST_ALPHA_OBJECT });
}

/// The tint every light's colour is scaled by in the device's light set, and the ambient's (the
/// log's `tints`: 0.5 and 0.5 on every draw of the frames), the constant term of a point light's
/// falloff, and the factor from a light's reach to its linear term (the logged `globals`: 1.0
/// and 0.1; a lamp's reach 2 is a k1 of 0.2).
pub const LIGHT_TINT: f32 = 0.5;
pub const AMBIENT_TINT: f32 = 0.5;
pub const POINT_K0: f32 = 1.0;
pub const POINT_RANGE_SCALE: f32 = 0.1;

/// A light colour as the game makes it a byte, back as 0..1 (`scene::lighting_of`): its tint times
/// the material's own factor for the channel.
fn tinted(colour: [f32; 3], tint: f32, material: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| crate::scene::byte(tint * material[i] * colour[i]))
}

/// A material's (diffuse, ambient) factors as the lights' tints take them (`Shader::material`; a
/// shader without them lights as 1: the window glass's diffuse is 0.2, the bathtub's 0.5, and the
/// log's light colours of those draws are the 0.5 tint times it).
pub type MaterialFactors = Option<([f32; 3], [f32; 3])>;

/// The device's light set for what the engine says lights an object, in the view the draw is
/// seen through: the ambient and sun (the direction the sun shines in, negated to point at it
/// and turned by the view) and each light's colour and place in view space.
pub fn lighting_for(l: &housefire::lighting::Lights, view: &Mat3x4, material: MaterialFactors) -> Lighting {
    let (diffuse, ambient_k) = material.unwrap_or(([1.0; 3], [1.0; 3]));
    let turn = |d: [f32; 3]| -> [f32; 3] { std::array::from_fn(|r| view[r * 4] * d[0] + view[r * 4 + 1] * d[1] + view[r * 4 + 2] * d[2]) };
    let directionals = l.sun.iter().map(|s| Directional { colour: tinted(s.colour, LIGHT_TINT, diffuse), to_light: turn(s.direction.map(|v| -v)) }).collect();
    let points = l.points.iter().map(|p| Point { colour: tinted(p.colour, LIGHT_TINT, diffuse), position: crate::scene::apply(view, p.at), k0: POINT_K0, k1: p.reach * POINT_RANGE_SCALE }).collect();
    Lighting::Lit { ambient: tinted(l.ambient, AMBIENT_TINT, ambient_k), directionals, points }
}

/// What the engine's own state gives for a frame: the lot's model draws.
pub struct Plan {
    pub models: std::rc::Rc<Models>,
    pub draws: Vec<Planned>,
    pub placements: Vec<housefire::scene::Placement>,
    /// The GUIDs of each placement's parts, by leader.
    pub guids: std::collections::BTreeMap<i16, Vec<u32>>,
    /// What the objects draw with, by GUID (`quickdata::object_resources`).
    pub resources: std::rc::Rc<std::collections::BTreeMap<u32, housefire::quickdata::ObjResources>>,
    /// The lot's wall and fence pieces.
    pub pieces: Vec<housefire::scene::WallPiece>,
    /// The shadow pass's draws (`drawlist::LotDraws::shadows`).
    pub shadows: Vec<Planned>,
    /// The lot's floor strips (`housefire::floors`), by room.
    pub floors: Vec<housefire::floors::Strip>,
    /// The lamps among the placements and their lights (`housefire::lamps`).
    pub lamps: Vec<housefire::lamps::Lamp>,
    pub lights: housefire::lighting::LotLights,
    /// The clock the light is made at (hour, minute, second).
    pub clock: (i32, i32, i32),
}

/// The lot's camera at `tick` as `scene::Camera` makes it from the parameters: the live ones of
/// the last 'c' record at or before the tick (the lot loaded from a save has no player input to
/// run the camera's easing from, so its parameters are the log's), the view from them and the
/// projection from the wanted distance (`docs/assets.md`, "The camera").
pub fn camera(log: &housefire::tree_log::Log, tick: u32) -> Option<(Mat3x4, [f32; 6])> {
    use housefire::scene::Camera;
    let c = log.cams.iter().rfind(|c| c.tick <= tick)?;
    let l = c.live;
    let cam = Camera { target: [l[0], l[1], l[2]], distance: l[3], rotation: l[4], tilt: l[5] };
    Some((cam.view(), Camera::projection(c.wanted[3])))
}

/// The disc's files the lot's draws are made from, read once: the models, what the objects draw
/// with (`quickdata::object_resources`), the floors' patterns, the shaders.
pub struct Lot {
    art: housefire::drawlist::LotArt,
    pub models: std::rc::Rc<Models>,
    pub resources: std::rc::Rc<std::collections::BTreeMap<u32, housefire::quickdata::ObjResources>>,
    patterns: Vec<housefire::quickdata::FloorPattern>,
    terrain: u32,
    pub shaders: HashMap<u32, Shader>,
}

impl Lot {
    /// `house` is the lot's house number (its `HouseData` row: the loaded lot does not keep it).
    pub fn new(disc: &housefire::disc::Disc, house: usize) -> Option<Lot> {
        let quick = disc.read("quickdat.arc")?;
        let image = |name: &str| housefire::arc::entries(&quick).unwrap_or_default().into_iter().find(|e| e.name == name).and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e)));
        let resources = housefire::arc::entries(&quick)?.into_iter().find(|e| e.name == "SimsObjects").and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e))).map(|img| housefire::quickdata::object_resources(&img))?;
        let patterns = image("SimsTiles").map(|img| housefire::quickdata::floor_patterns(&img)).unwrap_or_default();
        let terrain = image("SimsUI").map(|img| housefire::quickdata::houses(&img)).and_then(|h| h.get(house).map(|h| h.terrain)).unwrap_or(0);
        Some(Lot { art: housefire::drawlist::LotArt::read(disc, house), models: std::rc::Rc::new(Models::read(disc)), resources: std::rc::Rc::new(resources), patterns, terrain, shaders: housefire::shader::index(disc) })
    }

    /// The draws of the lot as `world` has it at `tick`, seen from the eye of `view`. The clock the
    /// light is made at is the world's own (globals 0, 5 and 6: the hour, minute and second).
    pub fn plan(&self, world: &housefire::world::World, tick: u32, view: &Mat3x4) -> Plan {
        // the eye: -R^T t of the view
        let eye = [0, 1].map(|i| -(0..3).map(|r| view[4 * r + i] * view[4 * r + 3]).sum::<f32>());
        let lot = housefire::drawlist::lot_draws_with(world, &self.resources, &self.models, &self.art, tick, Some(eye));
        let guids_of = |id: i16| world.find_object(id).into_iter().chain(world.objects().filter(|o| o.leader == id)).map(|o| o.guid).collect::<Vec<u32>>();
        let guids = lot.placements.iter().map(|p| (p.leader, guids_of(p.leader))).collect();
        let lamps = housefire::lamps::on_lot(&lot.placements, guids_of, &self.resources, &self.models.sources);
        let g = |i: usize| world.globals.get(i).copied().map_or(0, i32::from);
        let clock = (g(0), g(5), g(6));
        let lights = housefire::lighting::LotLights::build(world, &self.resources, &lamps, clock);
        let floors = housefire::floors::strips(&world.walls, housefire::light::GRID_SIZE as i32, &self.patterns, self.terrain, &lights, |id| self.shaders.get(&id).and_then(|s| s.material()));
        Plan { models: self.models.clone(), draws: lot.draws, placements: lot.placements, guids, resources: self.resources.clone(), pieces: lot.pieces, shadows: lot.shadows, floors, lamps, lights, clock }
    }
}

/// The save's neighbourhood file the lot is read from: the memory card's `G4ME69` file, or the
/// disc's default one.
pub fn save_file(disc: &housefire::disc::Disc, card: Option<&std::path::Path>) -> Option<Vec<u8>> {
    match card {
        Some(p) => {
            let mut bytes = std::fs::read(p).ok()?;
            bytes.truncate(1 << 22);
            housefire::save::memory_card_file(&bytes, "G4ME69").ok()
        }
        None => disc.read("Runtime/default.ngh"),
    }
}

/// The engine's lot (the disc's, or the card's save: `card` the memory card image; the lot the
/// log ran on, `housefire::lotpick`) and its draws for `tick`, seen from the eye of `view`. The
/// loaded lot is the world as the game starts the lot; the animation clock is the tick's.
pub fn plan(disc: &housefire::disc::Disc, card: Option<&std::path::Path>, log: &housefire::tree_log::Log, tick: u32, view: &Mat3x4, house: usize) -> Option<Plan> {
    use housefire::{library::Library, save, world::World};
    let ngh = save_file(disc, card)?;
    let lib = Library::bustin_out(disc);
    let (lots, _, _, best, _) = housefire::lotpick::pick_lot(&lib, &ngh, log);
    let mut world = World::new(&lib);
    world.load_lot(&lots[best]).ok()?;
    // the lot's globals as saved (its SIMI chunk): the clock among them
    world.globals = save::read_lot_globals(&ngh).ok().and_then(|g| g.get(best).cloned()).unwrap_or_default();
    let lot = Lot::new(disc, house)?;
    if std::env::var_os("HF_OBJECTS").is_some() {
        let list = log.lists.iter().rfind(|l| l.tick <= tick);
        for o in world.objects().filter(|o| o.on_lot() && !o.person) {
            println!("  object {} guid {:08x} at ({}, {}) level {} leader {} flags {:?} in the log's list {:?} fields {:?}", o.id(), o.guid, o.x16, o.y16, o.level, o.leader, o.data.get(housefire::world::DATA_FLAGS), list.map(|l| l.ids.contains(&o.id())), world.definition(o).map(|d| d.fields.clone()));
        }
    }
    // the clock: the lot's own as loaded, then one pass for each tick before this one, as `replay`
    // counts them: a pass the player's dialog stopped (the log's `stopped_at`) does not count
    for t in 1..tick {
        world.clock_pass(log.passes.get(&t).is_some_and(|p| p.stopped_at != 0));
    }
    Some(lot.plan(&world, tick, view))
}

/// What the engine makes of a frame, to be put in the original's order (`assemble`).
pub struct Engine {
    /// Each floor strip's draws and the room (batch) it is in.
    pub floors: Vec<(usize, FloorDraws)>,
    /// The lot's own opaque draws (the ground the shadows fall on): after the first batch's floors.
    pub lot: Vec<Made>,
    /// The shadow draws by room.
    pub shadows: Vec<Vec<Made>>,
    /// The model draws (objects, walls, the lot's blended ones): after all batches, the opaque
    /// ones first.
    pub objects: Vec<Made>,
    /// The Sims' draws from the engine's world (`sims`); with none, the log's stay.
    pub sims: Option<crate::sims::SimDraws>,
}

/// The room whose batch the log's Sims' shadows go in until the engine makes them (the home
/// lot's Sims are in the main rooms, room 1).
const SIMS_SHADOW_ROOM: usize = 1;

/// Puts the engine's draws in the original's order and takes the log's out of the frame, but for
/// the Sims' (their skinned parts, what they hold, their shadows) and the interface's. The order
/// (`docs/assets.md`, "The floors and the shadows"): for each room's batch its floors, in the
/// first batch the lot's own ground, the shadows (the Sims' in `SIMS_SHADOW_ROOM`'s), then the
/// floors again as a mask that clears the shadows' stencil; then the models. Returns how many log
/// draws it took out.
pub fn assemble(scene: &mut Scene, engine: Engine) -> usize {
    for d in &scene.frame.draws {
        let info = &mut scene.drawn[d.id as usize - 1];
        if matches!(info.class, Class::RigidStrip | Class::DisplayList) && (housefire::drawlist::is_sims(&info.model) || housefire::drawlist::is_held(&info.model)) {
            info.class = Class::Skinned;
        }
    }
    // with the Sims from the engine the log's Sims, shadows and portrait go; what they hold stays
    let engine_sims = engine.sims.is_some();
    let (mut kept, mut sims_shadows, mut removed) = (Vec::new(), Vec::new(), 0);
    for d in std::mem::take(&mut scene.frame.draws) {
        let info = &scene.drawn[d.id as usize - 1];
        match info.class {
            Class::Shadow if housefire::drawlist::is_sims(&info.model) => {
                if engine_sims {
                    removed += 1;
                } else {
                    sims_shadows.push(d)
                }
            }
            Class::Skinned if engine_sims && !housefire::drawlist::is_held(&info.model) => removed += 1,
            Class::Overlay if engine_sims => removed += 1,
            Class::RigidStrip | Class::DisplayList | Class::Floor | Class::FloorMask | Class::Shadow => removed += 1,
            _ => kept.push(d),
        }
    }
    let mut out: Vec<Draw> = Vec::new();
    let put = |scene: &mut Scene, out: &mut Vec<Draw>, mut m: Made| {
        scene.drawn.push(m.info.clone());
        scene.all.push(m.info);
        m.draw.id = scene.drawn.len() as u32;
        out.push(m.draw);
    };
    let sim_draws = engine.sims.unwrap_or_default();
    let mut sim_shadows = sim_draws.shadows;
    let rooms = engine.floors.iter().map(|f| f.0).chain(0..engine.shadows.len()).chain(sim_shadows.iter().map(|s| s.0 + 1)).max().map_or(0, |m| m + 1);
    let mut floors: Vec<Vec<FloorDraws>> = (0..rooms).map(|_| Vec::new()).collect();
    for (room, f) in engine.floors {
        floors[room].push(f);
    }
    let (mut lot, mut shadows) = (Some(engine.lot), engine.shadows.into_iter());
    let mut sims = Some(sims_shadows);
    for (room, batch) in floors.into_iter().enumerate() {
        let (mut masks, mut batch_floors) = (Vec::new(), Vec::new());
        for f in batch {
            batch_floors.push(f.floor);
            masks.push(f.mask);
        }
        for m in batch_floors {
            put(scene, &mut out, m);
        }
        for m in lot.take().into_iter().flatten() {
            put(scene, &mut out, m);
        }
        for m in shadows.next().unwrap_or_default() {
            put(scene, &mut out, m);
        }
        if room == SIMS_SHADOW_ROOM {
            out.extend(sims.take().into_iter().flatten());
        }
        // the engine's Sims' shadows of this room, after its other shadows
        for (r, m) in std::mem::take(&mut sim_shadows) {
            if r == room {
                put(scene, &mut out, m);
            } else {
                sim_shadows.push((r, m));
            }
        }
        for m in masks {
            put(scene, &mut out, m);
        }
    }
    out.extend(sims.take().into_iter().flatten());
    let (opaque, blended): (Vec<Made>, Vec<Made>) = engine.objects.into_iter().partition(|m| m.draw.material.blend == Blend::Opaque);
    // the Sims go between the opaque and the blended models (the original's order is spatial: a
    // window's glass that writes depth must come after the Sim behind it), the interface's last
    let (overlay, sims): (Vec<Draw>, Vec<Draw>) = kept.into_iter().partition(|d| d.projection.is_some());
    for m in opaque {
        put(scene, &mut out, m);
    }
    out.extend(sims);
    for m in sim_draws.house {
        put(scene, &mut out, m);
    }
    for m in blended {
        put(scene, &mut out, m);
    }
    out.extend(overlay);
    for m in sim_draws.portrait.into_iter().chain(sim_draws.leak) {
        put(scene, &mut out, m);
    }
    scene.frame.draws = out;
    removed
}

/// A window's frames from the engine's world, one after another: the scene keeps the meshes and
/// textures of every frame so far (`Caches`: each added once, for the caller to upload the new
/// ones), and each frame's draws are made from the world and the camera alone, in the original's
/// order. The Sims (and the interface) are not drawn yet.
pub struct Builder<'a> {
    assets: &'a Assets,
    lot: &'a Lot,
    caches: Caches,
    sim_cache: crate::sims::SimCaches,
    pub scene: Scene,
}

/// What a `Builder` frame was made of, for a log.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    /// The lot's planned draws, shadows and floor strips.
    pub planned: usize,
    pub shadows: usize,
    pub floors: usize,
    /// Draws in the frame.
    pub draws: usize,
}

impl<'a> Builder<'a> {
    pub fn new(assets: &'a Assets, lot: &'a Lot) -> Builder<'a> {
        let frame = Frame { camera: Camera { view: IDENTITY, projection: [0.0; 6] }, draws: Vec::new(), backdrop: None };
        Builder { assets, lot, caches: Caches { transient_floors: true, ..Caches::default() }, sim_cache: Default::default(), scene: Scene { frame, drawn: Vec::new(), all: Vec::new(), meshes: Vec::new(), transient: Vec::new(), textures: Vec::new() } }
    }

    /// The frame of `world` on `tick` seen through `camera` (a view and a projection): in
    /// `scene.frame`, with the meshes and textures it needs appended to the scene's.
    pub fn build(&mut self, world: &housefire::world::World, tick: u32, camera: (Mat3x4, [f32; 6])) -> Counts {
        self.build_with(world, tick, camera, None)
    }

    /// `build`, with the Sims of `sims` (the wardrobe, the figures and the Sim whose portrait is
    /// shown) drawn from the engine's state.
    pub fn build_with(&mut self, world: &housefire::world::World, tick: u32, camera: (Mat3x4, [f32; 6]), sims: Option<(&crate::sims::Wardrobe, &[crate::sims::Figure], Option<i16>)>) -> Counts {
        let (view, projection) = camera;
        let scene = &mut self.scene;
        scene.frame.draws.clear();
        scene.drawn.clear();
        scene.all.clear();
        scene.transient.clear();
        scene.frame.camera = Camera { view, projection };
        scene.frame.backdrop = None;
        let plan = self.lot.plan(world, tick, &view);
        let lighting = |d: &Planned, mat: MaterialFactors| lighting_for(&plan.lights.of(d), &view, mat);
        let made = self.assets.engine_draws(scene, &mut self.caches, &Inputs { draws: &plan.draws, models: &plan.models, shaders: &self.lot.shaders, lighting: &lighting, view });
        let floors: Vec<(usize, FloorDraws)> = plan.floors.iter().zip(self.assets.engine_floors(scene, &mut self.caches, &plan.floors, &self.lot.shaders, view)).map(|(st, f)| (st.room, f)).collect();
        let sun = plan.lights.at(1.0, 1.0).sun.map_or([0.0; 3], |s| s.direction);
        let by_room = self.assets.engine_shadows_by_room(scene, &mut self.caches, &plan, &view, &projection, sun);
        let (lot, objects): (Vec<Made>, Vec<Made>) = made.into_iter().partition(|m| plan.draws[m.from].kind == Kind::LotList && m.draw.material.blend == Blend::Opaque);
        let counts = Counts { planned: plan.draws.len(), shadows: plan.shadows.len(), floors: floors.len(), draws: 0 };
        let sims = sims.map(|(w, figures, portrait)| self.assets.engine_sims(scene, &mut self.caches, &mut self.sim_cache, w, figures, &plan.lights, sun, view, portrait));
        assemble(scene, Engine { floors, lot, shadows: by_room, objects, sims });
        Counts { draws: scene.frame.draws.len(), ..counts }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housefire::lighting::{Lights, Source, Sun};

    #[test]
    fn the_shadow_view_shears_height_onto_the_ground_along_the_sun() {
        // a view that is the identity: a point of height 1 (the z axis) lands at (a, b) on the
        // ground, the sun's x and y over the divisor
        let view: Mat3x4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let v = shadow_view(&view, [0.8, -0.2, -0.5], 0.8);
        assert_eq!([v[2], v[6], v[10]], [1.0, -0.25, 0.0]);
        // the ground itself (x, y columns) and the translation are the camera's
        assert_eq!([v[0], v[5], v[3]], [1.0, 1.0, 0.0]);
    }

    #[test]
    fn a_light_set_is_tinted_and_turned_into_the_view() {
        // a view turning x to -y (and y to x), moving by (0, 0, -10)
        let view: Mat3x4 = [0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, -10.0];
        let lights = Lights {
            ambient: [0.4; 3],
            sun: Some(Sun { colour: [1.0; 3], direction: [-1.0, 0.0, 0.0] }),
            points: vec![Source { at: [1.0, 2.0, 3.0], reach: 2.0, colour: [0.6; 3] }],
        };
        let Lighting::Lit { ambient, directionals, points } = lighting_for(&lights, &view, None) else { panic!("lit") };
        assert!((ambient[0] - 0.2).abs() < 0.003);
        // to the sun is the direction negated: +x, which the view turns to -y
        assert_eq!(directionals.len(), 1);
        assert!((directionals[0].to_light[1] + 1.0).abs() < 1e-6 && directionals[0].to_light[0].abs() < 1e-6);
        assert!((directionals[0].colour[0] - 0.5).abs() < 0.003);
        assert_eq!(points[0].position, [2.0, -1.0, -7.0]);
        assert!((points[0].k1 - 0.2).abs() < 1e-6);
    }

    #[test]
    fn the_alpha_tests_reference_is_the_shaders_float() {
        let mut m = Material { combine: Combine::Vertex, layers: [None, None], blend: Blend::Opaque, cull: Cull::None, depth_test: None, depth_write: false, alpha_test: None, write_colour: true, write_alpha: true, dst_alpha: None, layer1_from_position: None, sphere: None };
        state_of(&mut m, Kind::ObjectList, true, Some(0.4));
        assert_eq!(m.alpha_test.map(|a| a.refs[0]), Some(102));
        // no clear pixels in its texture: no test
        state_of(&mut m, Kind::ObjectList, false, Some(0.4));
        assert_eq!(m.alpha_test, None);
        assert_eq!(m.dst_alpha, Some(DST_ALPHA_OBJECT));
        state_of(&mut m, Kind::LotList, false, None);
        assert_eq!(m.dst_alpha, Some(DST_ALPHA_GROUND));
    }
}
