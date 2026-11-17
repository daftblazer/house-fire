//! The draw list of a lot: which model parts the original draws for the world's objects, walls and
//! the lot's own models, each with its world matrix, made from the engine's own state and the
//! disc's data (`docs/assets.md`, "The engine's draw list"). Deterministic and GPU-free: the
//! renderer draws what this lists.
//!
//! A draw is a part of a model record (`draw::PartIndex`) and the part's world matrix in the lot's
//! frame: 3 x 4, row by row, the translation last, its 3 x 3 the rotation times the record's
//! scale (the positions' 16-bit units folded in).

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::anim::Anim;
use crate::disc::Disc;
use crate::draw::{Part, PartIndex};
use crate::lamps::{self, Sources};
use crate::quickdata::ObjResources;
use crate::routing::Opening;
use crate::scene::{Form, Placement, WallPiece};
use crate::pose::Pose;
use crate::skeleton::{Mat4, Skeleton, IDENTITY};

/// The bit of a pose's flag word that hides its node: a model's part on a node whose idle pose has
/// it is not drawn (a plant's spare leaf sets, a lamp's lit shade). Found by the parts the
/// original's draws leave out (`docs/assets.md`, "The engine's draw list").
const NODE_HIDDEN: u32 = 8;

/// What a draw is part of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A part of an object's model drawn as a strip (one bone's).
    ObjectPart,
    /// An object's model drawn from its display list.
    ObjectList,
    /// A piece of the lot's walls or fences.
    WallPiece,
    /// One of the lot's own models (the house shell, the road).
    LotList,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::ObjectPart, Kind::ObjectList, Kind::WallPiece, Kind::LotList];

    pub fn name(self) -> &'static str {
        match self {
            Kind::ObjectPart => "rigid object part",
            Kind::ObjectList => "object display list",
            Kind::WallPiece => "wall piece",
            Kind::LotList => "lot display list",
        }
    }
}

/// A model part drawn at a world matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub kind: Kind,
    /// The part (`Part::index`: record, node, batch).
    pub part: (usize, usize, usize),
    /// 3 x 4, row by row.
    pub world: [f32; 12],
    /// The place on the lot (x, y, in tiles) of what the draw is part of: an object's, a wall
    /// piece's; the lot's own models, the origin. What the lights rank the draw by.
    pub origin: [f32; 2],
    /// The place (x, y, in tiles) of the middle of the part's mesh at `world`: what the lights
    /// of an object's part are found at (`origin` for a draw that is not an object's part).
    pub centre: [f32; 2],
    /// The object's leader id (0 for a wall piece or the lot's own model).
    pub object: i16,
    /// The shader (`shaders.arc` id) the part is drawn with in place of its sub-model's own: a
    /// wall face's is its pattern's.
    pub shader: Option<u32>,
    /// The two ends (x, y) of a wall piece's or fence's edge: its lights are made at them.
    pub edge: Option<[[f32; 2]; 2]>,
}

/// What the draws are made from on the disc: the model records, the skeletons and animations
/// of the objects' controllers, and the models each object dataset (`datasets.arc`'s `RD_` entries)
/// holds.
pub struct Models {
    pub parts: PartIndex,
    pub sources: Sources,
    /// A dataset's id (the archive entry's) to the ids of its models' records.
    pub datasets: BTreeMap<u32, Vec<u32>>,
}

impl Models {
    pub fn read(disc: &Disc) -> Models {
        let ds = disc.read("datasets.arc").unwrap_or_default();
        let datasets = crate::arc::entries(&ds).unwrap_or_default().iter().map(|e| (e.id, dataset_models(crate::arc::bytes(&ds, e)))).collect();
        Models { parts: PartIndex::new(&disc.read("models.arc").unwrap_or_default()), sources: Sources::read(disc), datasets }
    }
}

/// The ids of the model records in a dataset: its `Models` section is the name, a zero, the
/// number of models (a word), then for each its id (the CRC of its name), its size (a word), four
/// zero bytes and the record (the size's bytes), as for the dataset's other sections.
fn dataset_models(b: &[u8]) -> Vec<u32> {
    let word = |p: usize| b.get(p..p + 4).map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]));
    const NAME: &[u8] = b"Models\0";
    let Some(at) = b.windows(NAME.len()).position(|w| w == NAME) else { return Vec::new() };
    let mut p = at + NAME.len();
    let Some(count) = word(p) else { return Vec::new() };
    p += 4;
    let mut ids = Vec::new();
    for _ in 0..count {
        let (Some(id), Some(size)) = (word(p), word(p + 4)) else { break };
        ids.push(id);
        p += 12 + size as usize;
    }
    ids
}

/// A 4 x 4 (rows, translation last) as a world matrix: its 3 x 3 transposed (a column vector is
/// moved by it) and scaled by the record's.
pub fn world_of(m: &Mat4, scale: f32) -> [f32; 12] {
    let mut out = [0.0; 12];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 4 + c] = m[c * 4 + r] * scale;
        }
        out[r * 4 + 3] = m[12 + r];
    }
    out
}

/// The place (x, y) of the middle of a part's mesh (the mean of its vertices, in the record's
/// units) at `world`; the world's own place for a mesh with no 16-bit positions (the cursors').
pub fn mesh_centre(part: &Part<'_>, world: &[f32; 12]) -> [f32; 2] {
    let Some(m) = part.batch.centre else { return [world[3], world[7]] };
    [0, 1].map(|r| world[4 * r] * m[0] + world[4 * r + 1] * m[1] + world[4 * r + 2] * m[2] + world[4 * r + 3])
}

/// The part of the screen (0 to 1 across and down) a part's place may be in for the original to
/// draw it, as (left, right, top, bottom): a model reaches past its place (a tree's crown, a
/// wall's height). Fitted at tick 1,039 of home-a39.log (the log's draws' places span x 0.006 to
/// 1.034, y -0.179 to 0.620) with a little room: the smallest box that holds every draw the log
/// made. Not the original's own test (a bounding volume of the record against the frustum, not
/// decoded): the renderer clips what is left.
pub const VISIBLE: [f32; 4] = [0.0, 1.04, -0.19, 0.63];

/// Whether the place of a draw (its `world`'s translation) is in `VISIBLE` of the screen seen by
/// the camera with `view` and `projection`.
pub fn in_view(view: &[f32; 12], projection: &[f32; 6], world: &[f32; 12]) -> bool {
    crate::scene::Camera::on_screen(view, projection, [world[3], world[7], world[11]]).is_some_and(|s| (VISIBLE[0]..=VISIBLE[1]).contains(&s[0]) && (VISIBLE[2]..=VISIBLE[3]).contains(&s[1]))
}

/// The game's ticks a second (the animations' rates are frames a second).
const TICKS_PER_SECOND: f32 = 30.0;

/// The frame an object's idle animation is at on `tick`: it has played from tick 0 at its rate
/// (frames a second), looping (`Anim::end_mode` 0) or held at its last frame (`clock`).
pub fn idle_place(anim: &Anim, tick: u32) -> f32 {
    clock(anim.frames, anim.rate, anim.end_mode() == 0, tick)
}

/// The frame of an animation of `frames` frames at `rate` frames a second on `tick`: the ticks'
/// share of the rate and one frame more (the first tick's own), wrapped when it loops. The home
/// lot's trees (201 frames at 60 a second, looping) are at frame 69 on tick 1,039 and match the
/// original's draws on tick 1,040 too (`docs/assets.md`, "The engine's draw list").
fn clock(frames: u32, rate: f32, loops: bool, tick: u32) -> f32 {
    let at = tick as f32 * rate / TICKS_PER_SECOND + 1.0;
    if loops { at.rem_euclid(frames as f32) } else { at.min(frames.saturating_sub(1) as f32) }
}

/// What an object is playing: its graphic number (data field 0, which picks its animation among
/// its states), the render frames (the guest's display copies) since its script changed it, when
/// it did during the run, and the place its animation was found at when the log says (`anchor`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Play {
    pub graphic: i16,
    pub frames: Option<u32>,
    /// The animation's frame, from the lights the original logged: it wins over `frames`.
    pub anchor: Option<f32>,
    /// The same animation's frame at the light grid's last build (the lights' places the grid
    /// and the lit draws read): `anchor` less the display copies since.
    pub anchor_at_build: Option<f32>,
}

/// The objects' plays on a tick: those the run changed or kept, by id (an object not in it is as
/// the loaded lot has it, playing from the start of the run).
#[derive(Clone, Debug, Default)]
pub struct Plays {
    pub tick: u32,
    pub by_id: BTreeMap<i16, Play>,
}

/// The frames an animation of `rate` frames a second goes on by in one render frame (a display
/// copy, a game tick's frame): the game runs 30 of them a second.
pub fn frames_per_copy(rate: f32) -> f32 {
    rate / TICKS_PER_SECOND
}

/// An animation's frame `at` as it plays: wrapped when it loops, else held at its last.
pub fn wrap_place(anim: &Anim, at: f32) -> f32 {
    if anim.end_mode() == 0 { at.rem_euclid(anim.frames as f32) } else { at.min(anim.frames.saturating_sub(1) as f32) }
}

impl Plays {
    /// The plays of a lot as loaded, on `tick`.
    pub fn at(tick: u32) -> Plays {
        Plays { tick, by_id: BTreeMap::new() }
    }

    /// The graphic number of object `id`: the run's, else the loaded world's.
    pub fn graphic(&self, world: &crate::world::World, id: i16) -> i16 {
        self.by_id.get(&id).map_or_else(|| lamps::object_graphic(world, id), |p| p.graphic)
    }

    /// The frame the object's animation `anim` is at on `tick`: the anchored place, else from the
    /// render frames since its graphic was set, else from the start of the run.
    pub fn place(&self, id: i16, anim: &Anim) -> f32 {
        match self.by_id.get(&id) {
            Some(Play { anchor: Some(at), .. }) => *at,
            Some(Play { frames: Some(frames), .. }) => wrap_place(anim, *frames as f32 * frames_per_copy(anim.rate)),
            _ => idle_place(anim, self.tick),
        }
    }
}

impl Plays {
    /// The frame the animation was at when the light grid was last built, which is where the
    /// lamps' lights are for the grid and the lit draws (`place` when the log does not say).
    pub fn place_at_build(&self, id: i16, anim: &Anim) -> f32 {
        match self.by_id.get(&id) {
            Some(Play { anchor_at_build: Some(at), .. }) => *at,
            _ => self.place(id, anim),
        }
    }
}

fn kind_of(part: &Part<'_>) -> Kind {
    if part.batch.list.is_some() { Kind::ObjectList } else { Kind::ObjectPart }
}

/// The draws of the lot's objects: for each placement (one a multi-tile object's leader), the
/// parts of the models its parts' definitions name, on the bones of its idle pose. A part with several
/// matrices (a Sim's skin) is not an object's. An object plays its idle animation from the start
/// of the run (`idle_place` at `tick`).
pub fn object_draws(models: &Models, placements: &[Placement], guids_of: impl Fn(i16) -> Vec<u32>, resources: &BTreeMap<u32, ObjResources>, tick: u32) -> Vec<Draw> {
    object_parts(models, placements, guids_of, resources, &mut IdlePoses::default(), |_| 0, &Plays::at(tick)).into_iter().map(|p| p.0).collect()
}

/// What an object's idle animation gives for a frame (the bones' matrices and the poses).
type Posed = Rc<(Vec<Mat4>, Vec<Pose>)>;

/// The idle poses of the lot's objects, kept from frame to frame: an object that has not moved
/// and whose animation is at the same frame is posed the same, which is most of them.
#[derive(Default)]
pub struct IdlePoses {
    /// By the placement's leader and the model record: what the pose was made from (the skeleton
    /// and animation by their ids, the frame, the matrix and the scale, as bits) and the pose.
    last: HashMap<(i16, usize), (PoseKey, Option<Posed>)>,
}

type PoseKey = (u32, u32, u32, [u32; 16], u32);

impl IdlePoses {
    /// The pose of the idle animation `anim` on `skeleton` at `place`, from the last one of `at`
    /// when it was made from the same inputs. `ids` name the skeleton and the animation.
    fn pose(&mut self, at: (i16, usize), ids: (u32, u32), (skeleton, anim): (&Skeleton, &Anim), place: f32, (root, scale): (&Mat4, f32)) -> Option<Posed> {
        let key: PoseKey = (ids.0, ids.1, place.to_bits(), root.map(f32::to_bits), scale.to_bits());
        if let Some((k, posed)) = self.last.get(&at)
            && *k == key
        {
            return posed.clone();
        }
        let posed = lamps::bones_at(skeleton, anim, place, root, scale).map(Rc::new);
        self.last.insert(at, (key, posed.clone()));
        posed
    }
}

/// `object_draws`, each with whether the part is drawn at its object's own matrix (no skeleton
/// poses it).
fn object_parts(models: &Models, placements: &[Placement], guids_of: impl Fn(i16) -> Vec<u32>, resources: &BTreeMap<u32, ObjResources>, poses: &mut IdlePoses, graphic: impl Fn(i16) -> i16, plays: &Plays) -> Vec<(Draw, bool)> {
    let mut out = Vec::new();
    for p in placements {
        let guids = guids_of(p.leader);
        let root = lamps::object_matrix(p.x, p.y, p.angle);
        // each model the object's parts name, once (the bed's two halves have a model each)
        let mut drawn: Vec<usize> = Vec::new();
        let named: Vec<(&ObjResources, u32, usize)> = guids.iter().filter_map(|g| resources.get(g)).flat_map(|r| object_models(models, r).map(move |(id, record)| (r, id, record))).collect();
        for (r, id, record) in named {
            if drawn.contains(&record) {
                continue;
            }
            drawn.push(record);
            let rec = models.parts.record(record);
            let skeleton = models.sources.skeletons.get(&r.model).or_else(|| models.sources.skeletons.get(&id));
            let playing = r.animation(graphic(p.leader));
            let posed = skeleton.zip(models.sources.anims.get(&playing)).and_then(|(s, a)| poses.pose((p.leader, record), (r.model, playing), (s, a), plays.place(p.leader, a), (&root, rec.scale)));
            let bones = posed.as_ref().map(|b| &b.0);
            let hidden = |bone: Option<usize>| posed.as_ref().zip(bone).is_some_and(|(p, b)| p.1.get(b).is_some_and(|pose| pose.flags & NODE_HIDDEN != 0));
            for part in models.parts.parts_of(record).filter(|p| !p.skinned() && !hidden(p.bone())) {
                let m = match (part.bone(), &bones) {
                    (Some(b), Some(bones)) => bones.get(b).copied().unwrap_or(IDENTITY),
                    (None, Some(bones)) => bones.first().copied().unwrap_or(IDENTITY),
                    (_, None) => root,
                };
                // the root's own 3 x 3 is a pure turn: the record's scale is applied with the
                // bones' (`idle_bones`) or here
                let scale = if bones.is_some() { 1.0 } else { rec.scale };
                let world = world_of(&m, scale);
                out.push((Draw { kind: kind_of(&part), part: part.index, world, origin: [p.x, p.y], centre: mesh_centre(&part, &world), object: p.leader, shader: None, edge: None }, bones.is_none()));
            }
        }
    }
    out
}

/// The models an object's resources draw: the record its definition names (the model's hash, or
/// the first word of its record, the model's id in the object's dataset), else the records of its
/// dataset (each with its id and its place in the index).
fn object_models<'a>(models: &'a Models, r: &'a ObjResources) -> impl Iterator<Item = (u32, usize)> + 'a {
    // an object with no model and no dataset has only the placeholder record (`tom_debug`) as its
    // resources' first word: the original draws nothing for it
    // an object whose resources name no dataset (ResData flag 0x80000000: the parked scooter's
    // vehicle row, a model of `models.arc` alone) is not drawn with the lot's objects
    let placeholder = r.dataset == 0;
    // (the one the object's own dataset lists comes first: the club's pool table names the stock
    // table's model but its dataset lists the club's own, with the club's felt)
    let listed = models.datasets.get(&r.dataset);
    let found = |id: &&u32| models.parts.record_by_id(**id).is_some();
    let named = [&r.model, &r.key].into_iter().filter(found).find(|id| listed.is_some_and(|l| l.contains(id))).or_else(|| [&r.model, &r.key].into_iter().find(found));
    let ids: &[u32] = match named {
        _ if placeholder => &[],
        Some(id) => std::slice::from_ref(id),
        None => models.datasets.get(&r.dataset).map_or(&[], Vec::as_slice),
    };
    ids.iter().filter_map(|&id| Some((id, models.parts.record_by_id(id)?)))
}

/// The lot's own models: the house's outer lot model (`quickdata::House::outer_lot`), drawn at the
/// lot's origin.
pub fn lot_draws(models: &Models, outer_lot: u32) -> Vec<Draw> {
    let Some(record) = models.parts.record_by_id(outer_lot) else { return Vec::new() };
    let scale = models.parts.record(record).scale;
    models.parts.parts_of(record).map(|part| Draw { kind: Kind::LotList, part: part.index, world: world_of(&IDENTITY, scale), origin: [0.0; 2], centre: [0.0; 2], object: 0, shader: None, edge: None }).collect()
}

/// The wall models' sub-model that is not drawn in the lot's view (its hash, one of the two
/// 5-vertex pieces of each wall record: `docs/assets.md`, "The engine's draw list").
const UNDRAWN_WALL_SUB: u32 = 0x0906_522f;

/// The fewest vertices of a wall part that takes the wall's pattern: its face (18, 23 with a
/// door's or window's hole) and its foot (6; the log's 7-vertex draw, the same texture as the
/// face's) against the 5-vertex piece that keeps its own shader (the polyshadow's).
const WALL_FACE_VERTICES: usize = 6;

/// The models of the fence styles the lots use (the style the lot's wall array keeps, as
/// `SimsTiles`' `FenceData` rows number them): the home lot's picket fence (style 2). The other
/// styles' models are not seen yet.
pub const FENCE_MODELS: [(u8, &str); 1] = [(2, "Picket_Fence_straight")];

/// What the window object on a wall's edge is, by its model's name: the wall model has the matching
/// hole (`wall_straight_standard_window`, `_privacy_window`). The other windows of the catalogue
/// (plate glass, stain glass, heart) are not seen on the home lot.
pub const WINDOW_WALLS: [(&str, &str); 2] = [("_NEW_Single_Hung_Window", "standard_window"), ("_NEW_Privacy_Window", "privacy_window")];

/// The two ends of the edge a wall piece or fence is on: a part (at its tile's centre, turned a
/// with its back to the wall) has the wall at (centre + 0.5 (sin a, -cos a)), a fence stands on
/// the edge; both run along (cos a, sin a).
fn edge_of(p: &WallPiece, fence: bool) -> [[f32; 2]; 2] {
    let (s, c) = p.angle.to_radians().sin_cos();
    let mid = if fence { [p.x, p.y] } else { [p.x + 0.5 * s, p.y - 0.5 * c] };
    [[mid[0] - 0.5 * c, mid[1] - 0.5 * s], [mid[0] + 0.5 * c, mid[1] + 0.5 * s]]
}

/// A wall piece's matrix: the place and the turn about the vertical a degrees, a rotation (a
/// fence's, an object's, is mirrored: `lamps::object_matrix`).
fn wall_matrix(x: f32, y: f32, angle: f32) -> Mat4 {
    let (s, c) = angle.to_radians().sin_cos();
    let mut m = IDENTITY;
    m[0] = c;
    m[1] = s;
    m[4] = -s;
    m[5] = c;
    m[12] = x;
    m[13] = y;
    m
}

/// The records of the wall pieces' models, each found once: a model is looked up by a name made
/// of its opening, window and form (a format and a hash), and the lot's hundred pieces are of a
/// few dozen kinds. `window_of` gives the wall model's window name for the edge at a point
/// (`WINDOW_WALLS`).
struct WallRecords<'a, W> {
    models: &'a Models,
    window_of: W,
    /// By the opening (0 none, 1 door, 2 window), the window's name and the form.
    found: HashMap<(u8, &'static str, Form), Option<usize>>,
}

impl<'a, W: Fn([f32; 2]) -> Option<&'static str>> WallRecords<'a, W> {
    fn new(models: &'a Models, window_of: W) -> Self {
        WallRecords { models, window_of, found: HashMap::new() }
    }

    /// The record of a wall piece's model by its opening and its form.
    fn of(&mut self, p: &WallPiece) -> Option<usize> {
        let (opening, window) = match p.opening {
            Opening::None => (0, ""),
            Opening::Door => (1, ""),
            // the edge's middle, from the part's centre towards its wall
            Opening::Window => {
                let (s, c) = p.angle.to_radians().sin_cos();
                (2, (self.window_of)([p.x + 0.5 * s.round(), p.y - 0.5 * c.round()]).unwrap_or("standard_window"))
            }
        };
        let models = self.models;
        *self.found.entry((opening, window, p.form)).or_insert_with(|| {
            let infix = match opening {
                0 => String::new(),
                1 => "_door".to_string(),
                _ => format!("_{window}"),
            };
            let find = |form: Form| models.parts.record_by_id(crate::shader::name_hash(&format!("wall_straight{infix}{}", form.suffix())));
            // (a family without the form's model draws its plain one)
            find(p.form).or_else(|| find(Form::Straight))
        })
    }
}

/// The record of a diagonal wall's form (`wall_diagonal` and the forms' suffixes).
fn diagonal_record(models: &Models, form: Form) -> Option<usize> {
    let find = |form: Form| models.parts.record_by_id(crate::shader::name_hash(&format!("wall_diagonal{}", form.suffix())));
    find(form).or_else(|| find(Form::Straight))
}

/// The bits of an object's `ResData` flags that say it casts a shadow (0x02000000: furniture,
/// plants, the beds; 0x10000000: the sinks) and those that keep it from it (0x40000000: hung on a
/// wall; 0x20000000: the counters). Read off the home lot's 17 kinds of row against the original's
/// shadow draws (rows 0x01000000, the rug and the doors and windows, have neither bit set):
/// `docs/assets.md`, "The frame from the engine's state".
pub const SHADOW_CASTER_FLAGS: u32 = 0x0200_0000 | 0x1000_0000;
pub const NO_SHADOW_FLAGS: u32 = 0x4000_0000 | 0x2000_0000;

/// The shadow pass's draws of the lot's pieces: each fence and each wall piece on an outdoor tile
/// (`WallPiece::cap`: the outside of a wall casts on the ground, the inside of a house does not),
/// seen or not, draws every part of its model again (as strips, `Draw::part`'s vertex arrays)
/// with the shadow material; `window_of` as for `wall_draws`. The original draws these before the
/// objects' (found against home's 67 pieces, `docs/assets.md`).
pub fn wall_shadows(models: &Models, pieces: &[WallPiece], window_of: impl Fn([f32; 2]) -> Option<&'static str>) -> Vec<Draw> {
    let mut out = Vec::new();
    let mut records = WallRecords::new(models, window_of);
    for p in pieces.iter().filter(|p| p.fence || p.cap) {
        let (record, matrix) = if p.fence {
            let Some(name) = FENCE_MODELS.iter().find(|f| f.0 == p.pattern).map(|f| f.1) else { continue };
            let Some(record) = models.parts.record_by_id(crate::shader::name_hash(name)) else { continue };
            (record, lamps::object_matrix(p.x, p.y, p.angle))
        } else {
            // (the shadow is of the plain wall: an opening's hole is not in it)
            let Some(record) = records.of(&WallPiece { opening: Opening::None, ..*p }) else { continue };
            (record, wall_matrix(p.x, p.y, p.angle))
        };
        let world = world_of(&matrix, models.parts.record(record).scale);
        for part in models.parts.parts_of(record) {
            out.push(Draw { kind: Kind::WallPiece, part: part.index, world, origin: [p.x, p.y], centre: [p.x, p.y], object: 0, shader: None, edge: None });
        }
    }
    out
}

/// The shadow pass's draws of the diagonal walls: the plain `wall_diagonal` model (its forms that
/// reach past an end are not told apart, as `diagonal_draws`) at each diagonal tile's centre,
/// turned so that its face looks outdoors: the angle whose normal (see `diagonal_draws`) leads to
/// an outdoor tile.
pub fn diagonal_shadows(models: &Models, walls: &crate::routing::Walls, fences: &[u32]) -> Vec<Draw> {
    let mut out = Vec::new();
    let (w, h) = walls.size();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let bits = walls.diagonal_bits(x, y);
            if bits == 0 {
                continue;
            }
            let Some((angle, n)) = crate::scene::diagonal_faces(bits).into_iter().find(|(_, n)| !walls.indoors(x + n[0], y + n[1])) else { continue };
            let Some(record) = diagonal_record(models, crate::scene::diagonal_form(walls, x, y, n, fences)) else { continue };
            let scale = models.parts.record(record).scale;
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            for part in models.parts.parts_of(record) {
                out.push(Draw { kind: Kind::WallPiece, part: part.index, world: world_of(&wall_matrix(cx, cy, angle), scale), origin: [cx, cy], centre: [cx, cy], object: 0, shader: None, edge: None });
            }
        }
    }
    out
}

/// How far above the ground the shadow pass draws a part that its object's own matrix places (a
/// part on a skeleton's bone is where the bone is): 0.1 on every one of the home lot's (the
/// logged shadow draws' translations against the objects').
pub const SHADOW_LIFT: f32 = 0.1;

/// The shadow pass's draws of the objects: every part an object is drawn with, unless one of its
/// resource rows does not cast one (`SHADOW_CASTER_FLAGS`, `NO_SHADOW_FLAGS`). `parts` are the object draws (`object_parts`).
fn object_shadows(parts: &[(Draw, bool)], guids_of: impl Fn(i16) -> Vec<u32>, resources: &BTreeMap<u32, ObjResources>) -> Vec<Draw> {
    let mut casts: BTreeMap<i16, bool> = BTreeMap::new();
    parts
        .iter()
        .filter(|(d, _)| matches!(d.kind, Kind::ObjectPart | Kind::ObjectList))
        .filter(|(d, _)| *casts.entry(d.object).or_insert_with(|| guids_of(d.object).iter().filter_map(|g| resources.get(g)).all(|r| r.flags & SHADOW_CASTER_FLAGS != 0 && r.flags & NO_SHADOW_FLAGS == 0)))
        .map(|&(mut d, rooted)| {
            if rooted {
                d.world[11] += SHADOW_LIFT;
            }
            d
        })
        .collect()
}

/// The draws of the lot's walls and fences: each piece (`scene::wall_pieces`) draws its family's
/// model, by its opening (`window_of` gives the wall model's window name for the edge at a
/// point, `WINDOW_WALLS`) and its form, at its place. A wall part's sub-models but the undrawn
/// one are drawn when the part faces the eye (`WallPiece::faces`), the 6-vertex one whether the part
/// is indoors or not; a fence's only sub-model is drawn always.
pub fn wall_draws(models: &Models, pieces: &[WallPiece], patterns: &[u32], eye: [f32; 2], window_of: impl Fn([f32; 2]) -> Option<&'static str>) -> Vec<Draw> {
    let mut out = Vec::new();
    let mut records = WallRecords::new(models, window_of);
    for p in pieces {
        if p.fence {
            let Some(name) = FENCE_MODELS.iter().find(|f| f.0 == p.pattern).map(|f| f.1) else { continue };
            let Some(record) = models.parts.record_by_id(crate::shader::name_hash(name)) else { continue };
            let rec = models.parts.record(record);
            for part in models.parts.parts_of(record) {
                out.push(Draw { kind: Kind::WallPiece, part: part.index, world: world_of(&lamps::object_matrix(p.x, p.y, p.angle), rec.scale), origin: [p.x, p.y], centre: [p.x, p.y], object: 0, shader: None, edge: Some(edge_of(p, true)) });
            }
            continue;
        }
        let Some(record) = records.of(p) else { continue };
        let rec = models.parts.record(record);
        let world = world_of(&wall_matrix(p.x, p.y, p.angle), rec.scale);
        if !p.faces(eye) {
            continue;
        }
        for part in models.parts.parts_of(record).filter(|q| q.batch.hash != UNDRAWN_WALL_SUB) {
            // the face (the 18 or 23 vertex list) is drawn with the pattern's shader, the pieces
            // beside it (caps, 5 and 6 vertices) with their own
            let face = part.batch.list.is_some() && part.batch.count >= WALL_FACE_VERTICES;
            let shader = face.then(|| patterns.get(usize::from(p.pattern)).copied()).flatten();
            out.push(Draw { kind: Kind::WallPiece, part: part.index, world, origin: [p.x, p.y], centre: [p.x, p.y], object: 0, shader, edge: Some(edge_of(p, false)) });
        }
    }
    out
}

/// The draws of the lot's diagonal walls (a tile whose `Walls::diagonal_bits` is 0x10, a "/" from
/// (x, y + 1) to (x + 1, y), or 0x20, a "\\" from (x, y) to (x + 1, y + 1)): the family
/// `wall_diagonal` at the tile's centre, turned 0 or 180 for a "/" and 90 or -90 for a "\\", the
/// face drawn that turns to the eye, with the pattern of the side it faces, in the form its ends
/// ask for (`scene::diagonal_form`: `_left_extends`, `_right_extends`, `_both_extends`, the cap forms;
/// `docs/assets.md`, "The engine's draw list").
pub fn diagonal_draws(models: &Models, walls: &crate::routing::Walls, fences: &[u32], patterns: &[u32], eye: [f32; 2]) -> Vec<Draw> {
    let mut out = Vec::new();
    let (w, h) = walls.size();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let bits = walls.diagonal_bits(x, y);
            if bits == 0 {
                continue;
            }
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            // the line's ends: corners of the tile
            let ends = crate::scene::diagonal_ends(bits, x, y).map(|(ex, ey)| [ex as f32, ey as f32]);
            for (angle, n) in crate::scene::diagonal_faces(bits) {
                // a face is drawn when it turns to the eye (the 5 tiles seen on the home lot agree)
                if (n[0] as f32) * (eye[0] - cx) + (n[1] as f32) * (eye[1] - cy) <= 0.0 {
                    continue;
                }
                let form = crate::scene::diagonal_form(walls, x, y, n, fences);
                let Some(record) = diagonal_record(models, form) else { continue };
                let rec = models.parts.record(record);
                let world = world_of(&wall_matrix(cx, cy, angle), rec.scale);
                // the half tile it is on is across x (`light::cell_room_adds`): its room lights it.
                // Its face's pattern is the one the tile keeps on the sides opposite the face's
                // normal (the exterior siding at the corners seen: matched against the log's textures)
                let place = [cx + n[0] as f32, cy];
                let chosen = [walls.pattern(x, y, -n[0], 0), walls.pattern(x, y, 0, -n[1])].into_iter().find(|&p| p != 0).unwrap_or(0);
                let pattern = patterns.get(usize::from(chosen)).copied();
                for part in models.parts.parts_of(record).filter(|q| q.batch.hash != UNDRAWN_WALL_SUB) {
                    let face = part.batch.list.is_some() && part.batch.count >= WALL_FACE_VERTICES;
                    out.push(Draw { kind: Kind::WallPiece, part: part.index, world, origin: place, centre: place, object: 0, shader: face.then_some(pattern).flatten(), edge: Some(ends) });
                }
            }
        }
    }
    out
}

/// The window objects among the placements, each with the wall model's window name (the middle
/// of the wall's edge it sits on: a window object is drawn on it).
pub fn window_edges(models: &Models, placements: &[Placement], guids_of: impl Fn(i16) -> Vec<u32>, resources: &BTreeMap<u32, ObjResources>) -> Vec<([f32; 2], &'static str)> {
    placements
        .iter()
        .filter_map(|p| {
            let guids = guids_of(p.leader);
            let r = guids.iter().filter_map(|g| resources.get(g)).find(|r| object_models(models, r).next().is_some())?;
            let (_, record) = object_models(models, r).next()?;
            let name = &*models.parts.record(record).name;
            WINDOW_WALLS.iter().find(|w| w.0 == name).map(|w| ([p.x, p.y], w.1))
        })
        .collect()
}

/// Whether a record is a Sim's: an outfit's parts are named by two lowercase letters and an
/// underscore (am_ub_..., af_...), and the shadows `shadow_...`.
pub fn is_sims(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 3 && b[0].is_ascii_lowercase() && b[1].is_ascii_lowercase() && b[2] == b'_' || name.starts_with("shadow_")
}

/// Whether a record is an object a Sim carries (`Carry_...`, `Prop_...`) or the Sim's diamond: drawn
/// on the Sim's own bones, not the lot's.
pub fn is_held(name: &str) -> bool {
    name.starts_with("Carry_") || name.starts_with("Prop_") || name == "Diamond"
}

/// The lot's draws from the engine's world alone: its objects' (`object_draws`), its walls' (seen
/// from `eye`, `wall_draws`) and the house's own lot model (`lot_draws`), with the placements the
/// objects were drawn at.
pub struct LotDraws {
    pub draws: Vec<Draw>,
    pub placements: Vec<Placement>,
    pub pieces: Vec<WallPiece>,
    /// The shadow pass's draws (`object_shadows`, `wall_shadows`), in the order the original makes
    /// them: the pieces', then the objects'.
    pub shadows: Vec<Draw>,
    /// The GUIDs of each placement's parts, by leader.
    pub guids: BTreeMap<i16, Vec<u32>>,
}

/// What the lot's draws need from the disc's `quickdat.arc` besides the object resources, read
/// once for a house: the fences' styles, the wall patterns and the house's outer lot model.
pub struct LotArt {
    fences: Vec<u32>,
    patterns: Vec<u32>,
    outer_lot: Option<u32>,
}

impl LotArt {
    /// `house` is the lot's house number (its `HouseData` row: the loaded lot does not keep it).
    pub fn read(disc: &Disc, house: usize) -> LotArt {
        let quick = disc.read("quickdat.arc").unwrap_or_default();
        let image = |name: &str| crate::arc::entries(&quick).unwrap_or_default().into_iter().find(|e| e.name == name).and_then(|e| crate::quickdata::Image::read(crate::arc::bytes(&quick, &e)));
        let tiles = image("SimsTiles");
        LotArt {
            fences: tiles.as_ref().map(crate::quickdata::fence_styles).unwrap_or_default(),
            patterns: tiles.as_ref().map(crate::quickdata::wall_patterns).unwrap_or_default(),
            outer_lot: image("SimsUI").map(|img| crate::quickdata::houses(&img)).and_then(|h| h.get(house).map(|h| h.outer_lot)),
        }
    }
}

/// `res` is `quickdata::object_resources` of the disc's `SimsObjects`, `house` the lot's house
/// number (its `HouseData` row: the loaded lot does not keep it).
pub fn lot_draws_of(world: &crate::world::World, res: &BTreeMap<u32, ObjResources>, models: &Models, disc: &Disc, house: usize, tick: u32, eye: Option<[f32; 2]>) -> LotDraws {
    lot_draws_with(world, res, models, &LotArt::read(disc, house), eye, &mut IdlePoses::default(), &Plays::at(tick))
}

/// `lot_draws_of` with the disc's art read already (a caller that draws every tick).
pub fn lot_draws_with(world: &crate::world::World, res: &BTreeMap<u32, ObjResources>, models: &Models, art: &LotArt, eye: Option<[f32; 2]>, poses: &mut IdlePoses, plays: &Plays) -> LotDraws {
    let (fences, patterns, outer_lot) = (&art.fences, &art.patterns, art.outer_lot);
    let parts: Vec<crate::scene::Part> = world
        .objects()
        .filter(|w| w.on_lot() && !w.person)
        .map(|w| crate::scene::Part { id: w.id(), leader: if w.leader != 0 { w.leader } else { w.id() }, x16: w.x16, y16: w.y16, facing: i32::from(w.facing()), wall: res.get(&w.guid).is_some_and(|r| r.on_wall()) })
        .collect();
    let placements = crate::scene::placements(&parts);
    // each leader's parts' GUIDs: the leader itself, then the objects that name it, in the world's order
    let mut named: HashMap<i16, Vec<u32>> = HashMap::new();
    for o in world.objects() {
        named.entry(o.leader).or_default().push(o.guid);
    }
    let guids_of = |id: i16| world.find_object(id).map(|o| o.guid).into_iter().chain(named.get(&id).into_iter().flatten().copied()).collect::<Vec<u32>>();
    let guids: BTreeMap<i16, Vec<u32>> = placements.iter().map(|p| (p.leader, guids_of(p.leader))).collect();
    let parts = object_parts(models, &placements, guids_of, res, poses, |id| plays.graphic(world, id), plays);
    let mut draws: Vec<Draw> = parts.iter().map(|p| p.0).collect();
    let pieces = crate::scene::wall_pieces(&world.walls, 64, fences);
    let windows = window_edges(models, &placements, guids_of, res);
    let window_of = |at: [f32; 2]| windows.iter().find(|w| (w.0[0] - at[0]).abs() < 0.1 && (w.0[1] - at[1]).abs() < 0.1).map(|w| w.1);
    let mut shadows = wall_shadows(models, &pieces, window_of);
    shadows.extend(diagonal_shadows(models, &world.walls, fences));
    shadows.extend(object_shadows(&parts, guids_of, res));
    if let Some(eye) = eye {
        draws.extend(wall_draws(models, &pieces, patterns, eye, window_of));
        draws.extend(diagonal_draws(models, &world.walls, fences, patterns, eye));
    }
    draws.extend(outer_lot.map(|id| lot_draws(models, id)).unwrap_or_default());
    LotDraws { draws, placements, pieces, shadows, guids }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(object: i16, z: f32) -> Draw {
        let mut world = [0.0; 12];
        world[11] = z;
        Draw { kind: Kind::ObjectPart, part: (0, 0, 0), world, origin: [0.0; 2], centre: [0.0; 2], object, shader: None, edge: None }
    }

    #[test]
    fn an_object_casts_a_shadow_unless_hung_on_a_wall_or_a_floor_covering() {
        let row = |flags| ObjResources { dataset: 1, model: 0, flags, key: 0, idle: 0, states: Vec::new() };
        let resources: BTreeMap<u32, ObjResources> = [(1, row(0x0200_0000)), (2, row(0x4200_0000)), (3, row(0x0100_0000)), (4, row(0x1100_0000)), (5, row(0x2200_0000))].into();
        // object n has the resource row n
        let guids_of = |id: i16| vec![id as u32];
        let parts: Vec<(Draw, bool)> = (1..=5).map(|o| (draw(o, 0.0), false)).collect();
        let casters: Vec<i16> = object_shadows(&parts, guids_of, &resources).iter().map(|d| d.object).collect();
        assert_eq!(casters, vec![1, 4]);
    }

    #[test]
    fn a_part_on_its_objects_own_matrix_is_lifted_in_the_shadow_pass() {
        let resources: BTreeMap<u32, ObjResources> = [(1, ObjResources { dataset: 1, model: 0, flags: 0x0200_0000, key: 0, idle: 0, states: Vec::new() })].into();
        let parts = [(draw(1, 0.0), true), (draw(1, 0.075), false)];
        let shadows = object_shadows(&parts, |id| vec![id as u32], &resources);
        assert_eq!(shadows[0].world[11], SHADOW_LIFT);
        assert_eq!(shadows[1].world[11], 0.075);
    }

    #[test]
    fn a_dataset_lists_its_models_by_the_sections_sizes() {
        // "Models", a zero, 2 models: id, size 3, four zero bytes and 3 bytes of record each
        let mut b = b"RD_x\0\x04Textures\0\0\0\0\0Models\0".to_vec();
        b.extend_from_slice(&[0, 0, 0, 2]);
        for id in [0x11u32, 0x22] {
            b.extend_from_slice(&id.to_be_bytes());
            b.extend_from_slice(&[0, 0, 0, 3, 0, 0, 0, 0, 7, 7, 7]);
        }
        assert_eq!(dataset_models(&b), vec![0x11, 0x22]);
        assert!(dataset_models(b"no models here").is_empty());
    }

    #[test]
    fn a_world_matrix_is_the_rotation_transposed_and_scaled_with_the_translation_last() {
        let m = world_of(&lamps::object_matrix(14.5, 12.0, 0.0), 0.5);
        // turn 0 (facing 2): row 0 (0.5, ~0, 0), row 1 (~0, -0.5, 0), z up
        assert_eq!((m[0], m[5], m[10]), (0.5, -0.5, 0.5));
        assert_eq!((m[3], m[7], m[11]), (14.5, 12.0, 0.0));
    }

    #[test]
    fn a_wall_piece_turns_by_its_angle() {
        // the wall on a tile's +x side (90 degrees): row 0 is (0, -1), row 1 (1, 0)
        let m = world_of(&wall_matrix(13.5, 15.5, 90.0), 1.0);
        assert!(m[0].abs() < 1e-6 && (m[1] + 1.0).abs() < 1e-6 && (m[4] - 1.0).abs() < 1e-6 && m[5].abs() < 1e-6, "{m:?}");
        assert_eq!((m[3], m[7]), (13.5, 15.5));
    }

    #[test]
    fn a_wall_pieces_edge_runs_along_its_wall() {
        // the part on tile (13, 15) with its wall on the +x side (turned 90): the edge x = 14 from y 15 to 16
        let piece = WallPiece { x: 13.5, y: 15.5, angle: 90.0, fence: false, pattern: 0, cap: false, opening: Opening::None, form: Form::Straight };
        let e = edge_of(&piece, false);
        assert!((e[0][0] - 14.0).abs() < 1e-5 && (e[0][1] - 15.0).abs() < 1e-5, "{e:?}");
        assert!((e[1][0] - 14.0).abs() < 1e-5 && (e[1][1] - 16.0).abs() < 1e-5, "{e:?}");
        // a fence stands on the edge itself
        let fence = WallPiece { x: 14.5, y: 12.0, angle: 0.0, fence: true, ..piece };
        assert_eq!(edge_of(&fence, true), [[14.0, 12.0], [15.0, 12.0]]);
    }

    #[test]
    fn an_animation_clock_loops_from_the_first_tick() {
        // the trees' sway on tick 1,039 (home): 2 x 1,039 + 1 less 10 x 201
        assert_eq!(clock(201, 60.0, true, 1039), 69.0);
        assert_eq!(clock(201, 60.0, true, 0), 1.0);
        // one that stops holds its last frame
        assert_eq!(clock(10, 30.0, false, 100), 9.0);
    }
}
