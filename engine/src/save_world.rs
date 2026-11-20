//! A world written into a save, and read back: the engine's state as the save's models
//! (`LotObjects`, `objt`, the globals, the neighbourhood's data, the neighbours) and the card
//! they go into (`docs/lots.md`, "Saving the world").
//!
//! The save has values the engine does not model (a table entry's unnamed words, the section
//! header's, a Sim's +1136 member, ...): an object loaded from a lot keeps them (`Object::loaded`)
//! and they are written back as they were. An object made during play has none and gets the
//! rules of `new_object`.

use std::collections::BTreeMap;

use crate::goals::Goals;
use crate::iff::FormatError;
use crate::interp::Frame;
use crate::library::Library;
use crate::save::{
    LotChunks, LotGlobals, LotObject, LotObjects, NeighbourhoodChunk, ObjectType, RelationshipForm, SavedClass, SavedFrame,
    chunk_table, memory_card_file, parse_lot_globals, parse_neighbours, parse_ngbh, read_object_types,
};
use crate::save_write::{
    assemble_file, encode_chunk_table, encode_lot_globals, encode_lot_objects, encode_neighbours, encode_ngbh, encode_object_types, file_blocks, replace_card_file,
};
use crate::world::{ObjectKey, World};
use crate::{FileId, save};

/// The game's save file on a card.
pub const GAME: &str = "G4ME69";

/// An object's running stack, as the run keeps it (the simulator's), for the writer.
pub type Stacks<'a> = dyn FnMut(ObjectKey) -> Vec<Frame> + 'a;

/// The index the save gives a frame's file (`SavedFrame::file`) for a frame the lot did not load:
/// the objt type number of an object of that file (measured over the home card's 475 saved
/// frames: 474 name the object's own type; the one that does not, Sim 237's call of the counter's
/// tree 8259 in the meal's file, names the meal's type). The club's saved frames in other
/// objects' files (the dance floors', the bar's) name one type of the file's objects that is not
/// always the first or the last of the list, and not the stack object's: the rule is not found.
/// A frame in the object's own file takes its type, any other the type of the first object of
/// the list with that file.
fn file_kinds(world: &World, types: &[ObjectType]) -> BTreeMap<FileId, i16> {
    let mut map = BTreeMap::new();
    for o in world.objects() {
        let kind = o.loaded.as_ref().map(|l| l.kind).or_else(|| types.iter().find(|t| t.guid == o.guid).map(|t| t.fields[4] as i16));
        if let (Some(file), Some(kind)) = (o.file, kind) {
            map.entry(file).or_insert(kind);
        }
    }
    map
}

/// The frames of a running stack as the save holds them. The save's +8 word of frame k says the
/// frame above it (k + 1) was started by the engine (`World::load_lot` moves each up a frame).
fn saved_frames(lib: &Library, frames: &[Frame], loaded: &[SavedFrame], own: (Option<FileId>, i16), kinds: &BTreeMap<FileId, i16>) -> Vec<SavedFrame> {
    frames
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let was = loaded.get(k);
            let tree = f.tree.map_or_else(|| was.map_or(0, |w| w.tree), |t| lib.tree_at(t).id as u16);
            // (the top frame has none above it, so its word means nothing: kept as loaded)
            let engine_value = match frames.get(k + 1) {
                Some(n) if n.engine => was.map_or(1, |w| w.engine_value).max(1),
                Some(_) => 0,
                None => was.filter(|w| w.tree == tree).map_or(0, |w| w.engine_value),
            };
            // (a frame the lot loaded and the run left as it was keeps the file's index it had)
            let file = if let Some(w) = was.filter(|w| w.tree == tree && w.stack_object == f.stack_object) {
                w.file
            } else if f.file == own.0 { own.1 } else { f.file.and_then(|x| kinds.get(&x).copied()).unwrap_or(own.1) };
            SavedFrame { stack_object: f.stack_object, tree, node: f.node, params: f.params.clone(), locals: f.locals.clone(), engine: engine_value != 0, engine_value, file }
        })
        .collect()
}

/// The save's record of one object: what the engine has, over what the lot loaded with it.
fn saved_object(world: &World, k: ObjectKey, frames: &[Frame], types: &mut Vec<ObjectType>, kinds: &BTreeMap<FileId, i16>, serial: &mut i32) -> LotObject {
    let lib = world.library();
    let o = world.get(k);
    let mut s = match &o.loaded {
        Some(l) => (**l).clone(),
        None => new_object(world, k, types, serial),
    };
    s.id = o.id();
    // the header: the place and the box round it, moved with the object (the six words are x, y,
    // then two more points, then y and x of a third: a translation keeps what the lot saved)
    let (dx, dy) = (o.x16 - s.header[0], o.y16 - s.header[1]);
    for (i, v) in s.header[..6].iter_mut().enumerate() {
        *v += if i % 2 == 0 { dx } else { dy };
    }
    s.attributes = o.attributes.clone();
    s.temps = o.temps.to_vec();
    s.data = o.data.clone();
    s.data.resize(save::OBJECT_DATA_FIELDS, 0);
    s.stack = saved_frames(lib, frames, &s.stack, (o.file, s.kind), kinds);
    s.stack_capacity = s.stack_capacity.max(frames.len() as i32);
    s.relationships = o.relationships.clone();
    if let RelationshipForm::Sparse { flags, .. } = &s.relationship_form
        && flags.iter().filter(|&&f| f != 0).count() != s.relationships.len()
    {
        s.relationship_form = RelationshipForm::Counted;
    }
    if o.leader != 0 {
        s.leader = if o.leader == s.id { 0 } else { o.leader };
    }
    if s.saved_class == SavedClass::Sim {
        s.person_data = o.person_data.clone();
        s.person_data.resize(80, 0);
        s.motives = o.motives.clone();
        s.motives.resize(16, 0.0);
        s.motives_b = o.motives_b.clone();
        s.motives_b.resize(16, 0.0);
        s.current = o.current;
        // (the save keeps at most 8: its loader's limit)
        s.queue = o.queue.iter().take(8).copied().collect();
        s.motive_deltas = o.motive_deltas.clone();
    }
    s
}

/// An object the lot did not load (made during play): the save's table entry and section for it.
/// No recording has the game's save of a lot after play, so what it gives such an object is not
/// known. The rules here: its type is its GUID's in the lot's objt (a new one, numbered after the
/// last, if the lot has none; a Sim with no definition has its neighbour's GUID); the values the
/// engine does not model are those of an object of the same type the lot loaded (else of the
/// lot's first Sim, for a Sim; else zeros, a stack capacity of 512 and 65536 for the loader's
/// int: the values most objects of the lots have); its two serials (`a`, `b`) follow the lot's
/// highest, `b` one above `a` and the word after the pairs the `a` again, as in the objects of
/// the home card (a part of a multi-tile object has the leader's serial in `b`, not made here:
/// the engine makes no multi-tile objects).
fn new_object(world: &World, k: ObjectKey, types: &mut Vec<ObjectType>, serial: &mut i32) -> LotObject {
    let o = world.get(k);
    let d = world.definition(o);
    let saved_class = World::saved_class(d);
    let neighbour = o.person_data.get(crate::world::PERSON_NEIGHBOUR_ID).copied().filter(|&n| n != 0).and_then(|n| world.neighbours.iter().find(|m| m.id == n));
    let guid = if o.guid != 0 { o.guid } else { neighbour.map_or(0, |n| n.guid) };
    let kind = match types.iter().find(|t| t.guid == guid) {
        None if guid == 0 => 0, // (no type to name: the entry stays unnamed)
        Some(t) => t.fields[4] as i16,
        None => {
            // a new type: the next number after the lot's, its other fields those of the last
            let next = types.iter().map(|t| t.fields[4]).max().map_or(1, |m| m + 1);
            let mut fields = types.last().map_or([0; 6], |t| t.fields);
            fields[4] = next;
            types.push(ObjectType { guid, fields, name: String::new(), pad: 0 });
            next as i16
        }
    };
    let same_type = world.objects().filter_map(|t| t.loaded.as_deref().map(|l| (t, l))).find(|(t, l)| if guid != 0 { t.guid == guid } else { l.saved_class == saved_class });
    let mut s = match same_type {
        Some((_, l)) => l.clone(),
        None => LotObject { stack_capacity: 512, loader_int: 65536, header: [0, 0, 6, 6, 3, 3, 1, 1], ..Default::default() },
    };
    // the header relative to its own place (`saved_object` moves it to the object's)
    let (tx, ty) = (s.header[0], s.header[1]);
    for (i, v) in s.header[..6].iter_mut().enumerate() {
        *v -= if i % 2 == 0 { tx } else { ty };
    }
    s.id = o.id();
    s.kind = kind;
    s.saved_class = saved_class;
    (s.a, s.b, s.after_pairs) = (*serial, *serial + 1, *serial);
    *serial += 2;
    s.extra.clear();
    s.pairs.clear();
    s.shorts.clear();
    s.list_1180.clear();
    s.relationship_form = RelationshipForm::Counted;
    if matches!(saved_class, SavedClass::Part | SavedClass::Door) {
        s.leader = if o.leader == o.id() { 0 } else { o.leader };
    }
    s
}

/// The ObjM model of the world's objects, in the list's order: the lot's own part (the
/// `template`'s) after them. `types` is the lot's objt, which grows by the types of new objects.
pub fn lot_objects(world: &World, template: &LotObjects, types: &mut Vec<ObjectType>, stacks: &mut Stacks) -> LotObjects {
    let kinds = file_kinds(world, types);
    let mut out = LotObjects { objects: Vec::new(), ..template.clone() };
    let mut serial = world.objects().filter_map(|o| o.loaded.as_ref()).map(|l| l.a.max(l.b)).max().unwrap_or(0) + 1;
    for &k in world.order() {
        let frames = stacks(k);
        out.objects.push(saved_object(world, k, &frames, types, &kinds, &mut serial));
    }
    out
}

/// The neighbourhood's chunks written from the world: its data and the goals' marks, and the
/// neighbours as the world has them.
fn neighbourhood_chunk(world: &World, c: &[u8]) -> Result<Vec<u8>, FormatError> {
    let mut n: NeighbourhoodChunk = parse_ngbh(c)?;
    if world.neighbourhood.len() == n.words.len() {
        n.words = world.neighbourhood.clone();
    }
    if world.goals.met.len() == n.marks.len() {
        n.marks = world.goals.met.clone();
    }
    Ok(encode_ngbh(&n))
}

/// The neighbours with their Sims' motives written back: the load puts a house-less family's
/// neighbour motives into its Sims (`World::take_neighbour_motives`), so the save puts the Sims'
/// back into the neighbours (all but motive 11, which the load zeroes and the Sim does not keep).
fn neighbours_as_saved(world: &World) -> Vec<save::Neighbour> {
    let mut out = world.neighbours.clone();
    for o in world.objects().filter(|o| o.person && o.motives.len() == 16 && o.motives_b.len() == 16) {
        let nid = o.person_data.get(crate::world::PERSON_NEIGHBOUR_ID).copied().unwrap_or(0);
        let Some(n) = out.iter_mut().find(|n| n.id == nid && nid != 0 && n.house == Some(0) && n.motives.len() == 16 && n.motives_b.len() == 16) else { continue };
        for i in (0..16).filter(|&i| i != 11) {
            n.motives[i] = o.motives[i];
            n.motives_b[i] = o.motives_b[i];
        }
    }
    out
}

/// The save of the world on a card: `card` as the template (its file's other lots and sections
/// kept as they are), `lot` the place of the world's lot among the file's, written from the
/// world: its objects and types, its globals, the neighbourhood's data and goal marks, and the
/// neighbours. `stacks` gives each object's running stack.
pub fn save_card(world: &World, card: &[u8], lot: usize, stacks: &mut Stacks) -> Result<Vec<u8>, FormatError> {
    let file = memory_card_file(card, GAME)?;
    let mut blocks = file_blocks(&file)?;
    let mut lots_seen = 0;
    let mut simi_seen = 0;
    for b in &mut blocks {
        match b.section {
            0 => {
                let mut t = chunk_table(b)?;
                for (tag, c) in &mut t.chunks {
                    if tag == "SIMI" {
                        if simi_seen == lot
                            && let Some(g) = parse_lot_globals(c)
                        {
                            *c = encode_lot_globals(&LotGlobals { values: world.globals.clone(), ..g });
                        }
                        simi_seen += 1;
                    }
                }
                b.data = encode_chunk_table(&t, b.big_endian);
            }
            1 => {
                let mut t = chunk_table(b)?;
                for (tag, c) in &mut t.chunks {
                    match tag.as_str() {
                        "NGBH" => *c = neighbourhood_chunk(world, c)?,
                        "NBRS" => {
                            let mut table = parse_neighbours(c)?;
                            table.neighbours = neighbours_as_saved(world);
                            *c = encode_neighbours(&table);
                        }
                        _ => {}
                    }
                }
                b.data = encode_chunk_table(&t, b.big_endian);
            }
            3 => {
                if lots_seen == lot {
                    let mut t = chunk_table(b)?;
                    rewrite_lot(world, &mut t, stacks)?;
                    b.data = encode_chunk_table(&t, b.big_endian);
                }
                lots_seen += 1;
            }
            _ => {}
        }
    }
    let out = assemble_file(&file, &blocks)?;
    replace_card_file(card, GAME, &out)
}

/// A lot's ObjM and objt from the world.
fn rewrite_lot(world: &World, t: &mut LotChunks, stacks: &mut Stacks) -> Result<(), FormatError> {
    let objt = t.find("objt").ok_or_else(|| FormatError("a lot without objt".into()))?;
    let mut types = read_object_types(objt)?;
    let template = world.read_objects(t)?;
    let objects = lot_objects(world, &template, &mut types, stacks);
    for (tag, c) in &mut t.chunks {
        match tag.as_str() {
            "ObjM" => *c = encode_lot_objects(&objects),
            "objt" => *c = encode_object_types(&types),
            _ => {}
        }
    }
    Ok(())
}

/// A card's lot as it reads: its chunks, globals, the neighbourhood's neighbours, goal marks
/// and data (`Start::read` has the same, with the disc's goals besides).
pub struct CardLot {
    pub chunks: LotChunks,
    pub globals: Vec<i16>,
    pub neighbours: Vec<save::Neighbour>,
    pub goals: Goals,
    pub neighbourhood: Vec<i16>,
}

impl CardLot {
    pub fn read(card: &[u8], lot: usize) -> Result<CardLot, FormatError> {
        let file = memory_card_file(card, GAME)?;
        let chunks = save::read_lots(&file)?.get(lot).cloned().ok_or_else(|| FormatError("no such lot".into()))?;
        let globals = save::read_lot_globals(&file)?.get(lot).cloned().unwrap_or_default();
        let goals = Goals { houses: Vec::new(), met: save::read_goal_marks(&file)? };
        Ok(CardLot { chunks, globals, neighbours: save::read_neighbours(&file)?, goals, neighbourhood: save::read_neighbourhood_data(&file)? })
    }

    /// The world it loads as (what `replay` starts from, without the log's load pass): the
    /// lot's objects, its globals, the neighbours, the goals and the neighbourhood's data, with
    /// the load's own rules (the persons' sex, the visitors' mark, the household's motives from
    /// their neighbours).
    pub fn load<'l>(&self, lib: &'l Library, card_lot: bool) -> Result<World<'l>, FormatError> {
        let mut world = World::new(lib);
        world.load_lot(&self.chunks)?;
        world.globals = self.globals.clone();
        world.motives_run = card_lot;
        world.neighbours = self.neighbours.clone();
        world.describe_persons();
        world.mark_visitors();
        world.take_neighbour_motives();
        world.goals = self.goals.clone();
        world.neighbourhood = self.neighbourhood.clone();
        Ok(world)
    }
}

/// How the world differs from the one a card `saved` from it loads as (`differences`); the loaded
/// world's stacks are the lot's saved ones.
pub fn reload_differences(world: &World, stacks: &mut Stacks, saved: &[u8], lot: usize, card_lot: bool) -> Result<Vec<String>, FormatError> {
    let loaded = CardLot::read(saved, lot)?.load(world.library(), card_lot)?;
    let mut then = |k| loaded.get(k).stack.clone();
    Ok(differences(world, &loaded, stacks, &mut then))
}

/// How two worlds differ over everything a save holds: each differing field of each object (its
/// id and order, place, temps, data, attributes, person data, motives, deltas, relationships,
/// queue, stack), the globals, the neighbourhood's data, the goal marks and the neighbours. A
/// line each, naming what differs. A saved queue entry's engine-only fields (`interaction`,
/// `rank`) and the engine's own running state are not compared.
pub fn differences(a: &World, b: &World, sa: &mut Stacks, sb: &mut Stacks) -> Vec<String> {
    let mut out = Vec::new();
    let (oa, ob) = (a.order(), b.order());
    if oa.len() != ob.len() {
        out.push(format!("object count {} vs {}", oa.len(), ob.len()));
    }
    let lib = a.library();
    for (&ka, &kb) in oa.iter().zip(ob) {
        let (x, y) = (a.get(ka), b.get(kb));
        let id = x.id();
        let mut field = |name: &str, same: bool| {
            if !same {
                out.push(format!("object {id}: {name}"));
            }
        };
        field("id", x.id() == y.id());
        field("guid", x.guid == y.guid);
        field("place", (x.x16, x.y16) == (y.x16, y.y16));
        field("temps", x.temps == y.temps);
        field("data", x.data == y.data);
        field("attributes", x.attributes == y.attributes);
        field("leader", x.leader == y.leader);
        // (the load marks a household Sim's person data 32 as 0 and a visitor's 0 as 1 (`World::mark_visitors`): compared so)
        let household = x.person && x.person_data.get(crate::world::PERSON_NEIGHBOUR_ID).is_some_and(|&n| n != 0 && a.neighbours.iter().any(|m| m.id == n && (m.house == Some(0) || (m.house.is_some() && m.house == a.globals.get(crate::world::GLOBAL_HOUSE).map(|&h| i32::from(h))))));
        // (a Sim's lists are saved at their full length, 80 and 16: a made Sim's short ones read back padded with zeros)
        let sim = x.person || World::saved_class(a.definition(x)) == SavedClass::Sim;
        let pd = |d: &[i16]| {
            let mut v: Vec<i16> = d.to_vec();
            if sim {
                v.resize(80, 0);
            }
            if sim && let Some(m) = v.get_mut(crate::world::PERSON_VISITOR) {
                *m = if household { 0 } else if *m == 0 { 1 } else { *m };
            }
            v
        };
        field("person data", pd(&x.person_data) == pd(&y.person_data));
        let entry = |e: &save::SavedQueueEntry| (e.a, e.id, e.b, e.c, e.flags, e.objects, e.shorts, e.tree, e.e.to_bits());
        field("current entry", entry(&x.current) == entry(&y.current));
        field("queue", x.queue.len() == y.queue.len() && x.queue.iter().zip(&y.queue).all(|(p, q)| entry(p) == entry(q)));
        let frames = |fs: Vec<Frame>| fs.into_iter().map(|f| (f.tree.map_or(-1, |t| lib.tree_at(t).id as i32), f.node, f.params, f.locals, f.stack_object, f.engine)).collect::<Vec<_>>();
        let (fa, fb) = (frames(sa(ka)), frames(sb(kb)));
        field("stack", fa == fb);
    }
    if a.globals != b.globals {
        out.push("lot globals".into());
    }
    if a.neighbourhood != b.neighbourhood {
        out.push("neighbourhood data".into());
    }
    if a.goals.met != b.goals.met {
        out.push("goal marks".into());
    }
    // (the neighbours as they would be saved: with their Sims' motives)
    let saved = neighbours_as_saved(a);
    if saved.len() != b.neighbours.len() {
        out.push("neighbour count".into());
    }
    for (n, m) in saved.iter().zip(&b.neighbours) {
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        let rels = |n: &save::Neighbour| n.relationships.iter().map(|r| (r.with, r.values.clone(), r.word_d)).collect::<Vec<_>>();
        let same = (n.id, &n.name, n.guid, &n.person_data, n.house, n.slot) == (m.id, &m.name, m.guid, &m.person_data, m.house, m.slot) && bits(&n.motives) == bits(&m.motives) && bits(&n.motives_b) == bits(&m.motives_b) && rels(n) == rels(m);
        if !same {
            out.push(format!("neighbour {}", n.id));
        }
    }
    out
}
