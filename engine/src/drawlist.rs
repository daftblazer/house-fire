//! The draw list of a lot: which model parts the original draws for the world's objects, walls and
//! the lot's own models, each with its world matrix, made from the engine's own state and the
//! disc's data (`docs/assets.md`, "The engine's draw list"). Deterministic and GPU-free: the
//! renderer draws what this lists.
//!
//! A draw is a part of a model record (`draw::PartIndex`) and the part's world matrix in the lot's
//! frame: 3 x 4, row by row, the translation last, its 3 x 3 the rotation times the record's
//! scale (the positions' 16-bit units folded in).

use std::collections::BTreeMap;

use crate::disc::Disc;
use crate::draw::{Part, PartIndex};
use crate::lamps::{self, Sources};
use crate::quickdata::ObjResources;
use crate::routing::Opening;
use crate::scene::{Form, Placement, WallPiece};
use crate::skeleton::{Mat4, IDENTITY};

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

fn kind_of(part: &Part<'_>) -> Kind {
    if part.batch.list.is_some() { Kind::ObjectList } else { Kind::ObjectPart }
}

/// The draws of the lot's objects: for each placement (one a multi-tile object's leader), the
/// parts of the models its parts' definitions name, on the bones of its idle pose. A part with several
/// matrices (a Sim's skin) is not an object's.
pub fn object_draws(models: &Models, placements: &[Placement], guids_of: impl Fn(i16) -> Vec<u32>, resources: &BTreeMap<u32, ObjResources>) -> Vec<Draw> {
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
            let posed = skeleton.zip(models.sources.anims.get(&r.idle)).and_then(|(s, a)| lamps::idle_bones(s, a, &root, rec.scale));
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
                out.push(Draw { kind: kind_of(&part), part: part.index, world: world_of(&m, scale) });
            }
        }
    }
    out
}

/// The models an object's resources draw: the record its definition names (the model's hash, or
/// the first word of its record, the model's id in the object's dataset), else the records of its
/// dataset (each with its id and its place in the index).
fn object_models<'a>(models: &'a Models, r: &ObjResources) -> impl Iterator<Item = (u32, usize)> + 'a {
    let named = [r.model, r.key].into_iter().find(|&id| models.parts.record_by_id(id).is_some());
    let ids: Vec<u32> = match named {
        Some(id) => vec![id],
        None => models.datasets.get(&r.dataset).cloned().unwrap_or_default(),
    };
    ids.into_iter().filter_map(|id| Some((id, models.parts.record_by_id(id)?)))
}

/// The lot's own models: the house's outer lot model (`quickdata::House::outer_lot`), drawn at the
/// lot's origin.
pub fn lot_draws(models: &Models, outer_lot: u32) -> Vec<Draw> {
    let Some(record) = models.parts.record_by_id(outer_lot) else { return Vec::new() };
    let scale = models.parts.record(record).scale;
    models.parts.parts_of(record).map(|part| Draw { kind: Kind::LotList, part: part.index, world: world_of(&IDENTITY, scale) }).collect()
}

/// The wall models' sub-model that is not drawn in the lot's view (its hash, one of the two
/// 5-vertex pieces of each wall record: `docs/assets.md`, "The engine's draw list").
const UNDRAWN_WALL_SUB: u32 = 0x0906_522f;

/// The models of the fence styles the lots use (the style the lot's wall array keeps, as
/// `SimsTiles`' `FenceData` rows number them): the home lot's picket fence (style 2). The other
/// styles' models are not seen yet.
pub const FENCE_MODELS: [(u8, &str); 1] = [(2, "Picket_Fence_straight")];

/// What the window object on a wall's edge is, by its model's name: the wall model has the matching
/// hole (`wall_straight_standard_window`, `_privacy_window`). The other windows of the catalogue
/// (plate glass, stain glass, heart) are not seen on the home lot.
pub const WINDOW_WALLS: [(&str, &str); 2] = [("_NEW_Single_Hung_Window", "standard_window"), ("_NEW_Privacy_Window", "privacy_window")];

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

/// The draws of the lot's walls and fences: each piece (`scene::wall_pieces`) draws its family's
/// model, by its opening (`window_of` gives the wall model's window name for the edge at a
/// point, `WINDOW_WALLS`) and its form, at its place. A wall part's sub-models but the undrawn
/// one are drawn when the part faces the eye (`WallPiece::faces`), the 6-vertex one whether the part
/// is indoors or not; a fence's only sub-model is drawn always.
pub fn wall_draws(models: &Models, pieces: &[WallPiece], eye: [f32; 2], window_of: impl Fn([f32; 2]) -> Option<&'static str>) -> Vec<Draw> {
    let mut out = Vec::new();
    for p in pieces {
        if p.fence {
            let Some(name) = FENCE_MODELS.iter().find(|f| f.0 == p.pattern).map(|f| f.1) else { continue };
            let Some(record) = models.parts.record_by_id(crate::shader::name_hash(name)) else { continue };
            let rec = models.parts.record(record);
            for part in models.parts.parts_of(record) {
                out.push(Draw { kind: Kind::WallPiece, part: part.index, world: world_of(&lamps::object_matrix(p.x, p.y, p.angle), rec.scale) });
            }
            continue;
        }
        let opening = match p.opening {
            Opening::None => String::new(),
            Opening::Door => "_door".to_string(),
            // the edge's middle, from the part's centre towards its wall
            Opening::Window => {
                let (s, c) = p.angle.to_radians().sin_cos();
                format!("_{}", window_of([p.x + 0.5 * s.round(), p.y - 0.5 * c.round()]).unwrap_or("standard_window"))
            }
        };
        let form = match p.form {
            Form::Straight => "",
            Form::LeftExtends => "_left_extends",
            Form::RightExtends => "_right_extends",
        };
        let Some(record) = models.parts.record_by_id(crate::shader::name_hash(&format!("wall_straight{opening}{form}"))) else { continue };
        let rec = models.parts.record(record);
        let world = world_of(&wall_matrix(p.x, p.y, p.angle), rec.scale);
        if !p.faces(eye) {
            continue;
        }
        for part in models.parts.parts_of(record).filter(|q| q.batch.hash != UNDRAWN_WALL_SUB) {
            out.push(Draw { kind: Kind::WallPiece, part: part.index, world });
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
            let name = &models.parts.record(record).name;
            WINDOW_WALLS.iter().find(|w| w.0 == name).map(|w| ([p.x, p.y], w.1))
        })
        .collect()
}
