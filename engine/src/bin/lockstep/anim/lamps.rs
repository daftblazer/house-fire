//! The lamps' lights at their objects' posed nodes (LOCKSTEP_ANIM_LAMPS, stage 4.4f): each lamp
//! light the original placed ('1', Obj_PlaceLights 0x8007A8CC: the light's position after, the
//! node's 16 words) against the bone matrices ('<') of the same tick, and then against the
//! engine's own lamps (`lamps::on_lot`: the lot's objects, their skeletons, idle animations and
//! poses, no logged place).
//!
//! The object keeps its controller at +848 (the '<' records' controller is the '1' record's
//! object + 848), so a lamp's light is found by its object.
//!
//! Run (home, then the club, whose log has no bones):
//!   LOCKSTEP_ANIM_LAMPS=1 lockstep work/logs/home-a28.log ISO --card <mom-live card>
//!   LOCKSTEP_ANIM_LAMPS=1 lockstep work/logs/club-a1.log ISO --card <club-live card>
//! Lines: the lights against the bones; "object roots from the lot"; "lamp places from the
//! engine"; the lamp objects' final lights (any log). The window fills, the light grid's cells
//! and the light grid sums from the engine's lamps are lockstep's window lights study
//! (`set_to_next.rs`, `lamps_from_the_engine`):
//!   LOCKSTEP_DUMP_PLACES=1000 LOCKSTEP_DUMP_OUT=OUT LOCKSTEP_DUMP_WALLS=OUT2 \
//!   LOCKSTEP_WINDOW_LIGHTS=work/logs/home-a28.log lockstep work/logs/home.log ISO --card ...
//! (the club: LOCKSTEP_WINDOW_LIGHTS=work/logs/club-rooms.log on club.log).

use crate::*;
use housefire::lamps::{self, Lamp, Sources};
use housefire::tree_log::{BonesRec, Log};

/// Where the controller lies in an object (the '<' records' controller minus the '1' records'
/// object).
const CONTROLLER_OFFSET: u32 = 848;

fn f(v: u32) -> f32 {
    f32::from_bits(v)
}

/// The engine's lamps of the lot: its objects' placements (`scene::placements`), the resources
/// (`quickdata::object_resources`) and the disc's skeletons and animations.
pub fn engine_lamps(world: &World, res: &BTreeMap<u32, housefire::quickdata::ObjResources>, disc: &Disc) -> Vec<Lamp> {
    let parts: Vec<housefire::scene::Part> = world
        .objects()
        .filter(|w| w.on_lot() && !w.person)
        .map(|w| housefire::scene::Part { id: w.id(), leader: if w.leader != 0 { w.leader } else { w.id() }, x16: w.x16, y16: w.y16, facing: i32::from(w.facing()), wall: res.get(&w.guid).is_some_and(|r| r.on_wall()) })
        .collect();
    lamps::on_lot(&housefire::scene::placements(&parts), |id| world.find_object(id).into_iter().chain(world.objects().filter(|o| o.leader == id)).map(|o| o.guid).collect(), |id| lamps::object_graphic(world, id), res, &Sources::read(disc))
}

/// The engine's lamps as the run has them at `tick`: the log replayed natively (`Replay`) to it,
/// so that the objects' graphics (a lamp's switch, the dance floor's mode) are what the scripts
/// set by then. `lot` is the replay's lot index (`--lot`, -1 the default), `card` its save.
pub fn engine_lamps_at(log: &housefire::tree_log::Log, lib: &housefire::library::Library, card: Option<&str>, lot: i64, res: &BTreeMap<u32, housefire::quickdata::ObjResources>, disc: &Disc, tick: u32) -> Vec<Lamp> {
    use housefire::replay::{Replay, RunWith, Start};
    let Ok(start) = Start::read(disc, card, lot) else { return Vec::new() };
    let Some(mut replay) = Replay::new(log, lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, goals: start.goals.clone(), cat: None }) else { return Vec::new() };
    while replay.tick() < tick && replay.step() {}
    engine_lamps(replay.world(), res, disc)
}

/// The squared distance between two places.
fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

pub fn lamps(log: &Log, world: &World, res: &BTreeMap<u32, housefire::quickdata::ObjResources>, d: &Disc) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let ticks: BTreeSet<u32> = log.object_lights.iter().map(|o| o.tick).collect();
    println!("lamp records: {} over ticks {:?}..{:?}; bone records over ticks {:?}..{:?}", log.object_lights.len(), ticks.first(), ticks.last(), log.bones.first().map(|b| b.tick), log.bones.last().map(|b| b.tick));
    for o in &log.object_lights {
        for l in o.lights.iter().filter(|l| l.kind == 0) {
            let near: Vec<&BonesRec> = log.bones.iter().filter(|b| b.tick.abs_diff(o.tick) <= 1).collect();
            if near.is_empty() {
                *counts.entry("no bones that tick".into()).or_default() += 1;
                continue;
            }
            let at_bone = near.iter().any(|b| b.matrices.iter().any(|m| [f(m[12]), f(m[13]), f(m[14])] == l.at));
            let node_words = near.iter().any(|b| b.states.iter().any(|s| s[5..8] == l.words[5..8]));
            let node_is_at = [f(l.words[5]), f(l.words[6]), f(l.words[7])] == l.at;
            *counts.entry(format!("place a bone's translation: {at_bone}; node words a state's: {node_words}; place = node words 5-7: {node_is_at}")).or_default() += 1;
        }
    }
    println!("lamp lights against the bones: {counts:?}");

    // the controller link: each lamp object's controller has bone records
    let objects: BTreeSet<u32> = log.object_lights.iter().map(|o| o.object).collect();
    let with_bones = objects.iter().filter(|&&o| log.bones.iter().any(|b| b.controller == o + CONTROLLER_OFFSET)).count();
    println!("lamp objects {}: with bone records at object + {CONTROLLER_OFFSET}: {with_bones}", objects.len());

    // 1. the object roots from the lot: each lamp object's last root before its lights' last
    // tick (the lot's objects are placed in the first ticks) against the engine's matrix
    let mine = engine_lamps(world, res, d);
    let last_tick = ticks.last().copied().unwrap_or(0);
    let mut matched: BTreeMap<u32, usize> = BTreeMap::new();
    let (mut roots, mut roots_different, mut unmatched) = (0, 0, 0);
    for &o in &objects {
        let Some(b) = log.bones.iter().rfind(|b| b.controller == o + CONTROLLER_OFFSET && b.tick <= last_tick) else { continue };
        // the engine's lamp at the logged root's tile, with as many bones as its controller
        let (rx, ry) = (f(b.root[12]), f(b.root[13]));
        let sk_nodes = b.states.len();
        let Some(k) = mine.iter().position(|l| (l.place.x - rx).abs() < 0.01 && (l.place.y - ry).abs() < 0.01 && l.lights.iter().all(|x| x.node < sk_nodes)) else {
            unmatched += 1;
            continue;
        };
        matched.insert(o, k);
        let m = lamps::object_matrix(mine[k].place.x, mine[k].place.y, mine[k].place.angle);
        roots += 1;
        let same = m.iter().zip(&b.root).all(|(a, w)| *a == f(*w));
        roots_different += usize::from(!same);
        if !same {
            println!("  root of object {o:08x}: mine {m:?} the log's {:?}", b.root.map(f));
        }
    }
    println!("object roots from the lot ({} lamp objects, {} engine lamps, {unmatched} found at no engine lamp): DIFFERENT -> {roots_different} of {roots}", objects.len(), mine.len());

    // 2. the lamps' places from the engine: each kind-0 light's place, colour and reach against
    // the engine's lamp light for its node, once the object stands on its tile (a logged root
    // that is the identity is the object before it is placed: the original's own state then)
    let (mut placed, mut at_different, mut colour_different, mut before, mut no_node) = (0, 0, 0, 0, 0);
    let (mut shown, mut worst) = (0, 0.0f32);
    for o in &log.object_lights {
        let Some(&k) = matched.get(&o.object) else { continue };
        for l in o.lights.iter().filter(|l| l.kind == 0) {
            let Some(mine) = mine[k].lights.iter().find(|x| x.node == l.node as usize) else {
                no_node += 1;
                continue;
            };
            // the object's matrix when the light was placed: the bone record of that tick whose
            // matrices hold the light's place
            let on_tile = log
                .bones
                .iter()
                .filter(|b| b.controller == o.object + CONTROLLER_OFFSET && b.tick.abs_diff(o.tick) <= 1 && b.matrices.iter().any(|m| [f(m[12]), f(m[13]), f(m[14])] == l.at))
                .any(|b| b.root[12] != 0 || b.root[13] != 0);
            if !on_tile {
                before += 1;
                continue;
            }
            placed += 1;
            let at = l.at.map(f32::to_bits) == mine.at.map(f32::to_bits);
            let (colour, reach) = housefire::light::lamp_light([f(l.words[11]), f(l.words[12]), f(l.words[13])], f(l.words[14]), f(l.words[15]));
            let same_colour = mine.animated || colour.map(f32::to_bits) == mine.colour.map(f32::to_bits) && reach.to_bits() == mine.reach.to_bits();
            at_different += usize::from(!at);
            if !at {
                let e = mine.at.iter().zip(&l.at).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
                worst = worst.max(e);
            }
            colour_different += usize::from(!same_colour);
            if (!at || !same_colour) && shown < 8 {
                shown += 1;
                println!("  tick {} object {:08x} node {}: place mine {:?} the log's {:?}; colour mine {:?} {} the log's {colour:?} {reach}", o.tick, o.object, l.node, mine.at, l.at, mine.colour, mine.reach);
            }
        }
    }
    println!("lamp places from the engine ({placed} lights on their tiles, {before} before the object is placed, {no_node} at no engine light node): DIFFERENT -> {at_different} (place; the worst {worst:e}), {colour_different} (colour and reach)");

    // 3. the lamps' final places without the bones (the clubs' logs have none): each lamp
    // object's last record's kind-0 lights against the engine's lights, each place bit for bit
    // some engine lamp light's, and its colour and reach those of the animation its graphic picks (the
    // save's graphic: a lamp the run switches since is in another state)
    let mut last: BTreeMap<u32, &housefire::tree_log::ObjectLightsRec> = BTreeMap::new();
    for o in &log.object_lights {
        last.insert(o.object, o);
    }
    let all: Vec<(usize, &housefire::lamps::LampLight)> = mine.iter().enumerate().flat_map(|(k, l)| l.lights.iter().map(move |x| (k, x))).collect();
    // the animation (any of the disc's of the object's skeleton) that puts a lamp's light where
    // and as the log has it: the lamp's other states, by name
    let sources = Sources::read(d);
    let mut states: BTreeMap<String, usize> = BTreeMap::new();
    let mut state_of = |lamp: &Lamp, l: &housefire::tree_log::ObjectLight| {
        let skeleton = world.objects().filter(|o| o.leader == lamp.leader || o.id() == lamp.leader).filter_map(|o| res.get(&o.guid)).find_map(|r| sources.skeletons.get(&r.model));
        let Some(skeleton) = skeleton else { return };
        let root = lamps::object_matrix(lamp.place.x, lamp.place.y, lamp.place.angle);
        let (colour, reach) = housefire::light::lamp_light([f(l.words[11]), f(l.words[12]), f(l.words[13])], f(l.words[14]), f(l.words[15]));
        let names: Vec<&str> = sources
            .anims
            .values()
            .filter(|a| a.nodes.len() == skeleton.bones.len())
            .filter(|a| lamps::lights(skeleton, a, &root).iter().any(|m| m.node == l.node as usize && m.at.map(f32::to_bits) == l.at.map(f32::to_bits) && m.colour.map(f32::to_bits) == colour.map(f32::to_bits) && m.reach.to_bits() == reach.to_bits()))
            .map(|a| a.name.as_str())
            .collect();
        *states.entry(if names.is_empty() { format!("{}: no animation of the disc's", skeleton.name) } else { format!("{}: {}", skeleton.name, names.join(", ")) }).or_default() += 1;
    };
    let (mut lights, mut found, mut idle, mut animated, mut other_state) = (0, 0, 0, 0, 0);
    let mut used = BTreeSet::new();
    for o in last.values() {
        for l in o.lights.iter().filter(|l| l.kind == 0) {
            lights += 1;
            let Some(k) = all.iter().position(|m| m.1.at.map(f32::to_bits) == l.at.map(f32::to_bits)) else {
                println!("  no engine light at {:?} (object {:08x}, node {}); the engine's nearest {:?}", l.at, o.object, l.node, all.iter().min_by(|a, b| dist(a.1.at, l.at).total_cmp(&dist(b.1.at, l.at))).map(|m| (m.1.node, m.1.at)));
                if let Some(m) = all.iter().min_by(|a, b| dist(a.1.at, l.at).total_cmp(&dist(b.1.at, l.at))) {
                    state_of(&mine[m.0], l);
                }
                continue;
            };
            found += 1;
            used.insert(k);
            let (colour, reach) = housefire::light::lamp_light([f(l.words[11]), f(l.words[12]), f(l.words[13])], f(l.words[14]), f(l.words[15]));
            if all[k].1.animated {
                animated += 1;
            } else if colour.map(f32::to_bits) == all[k].1.colour.map(f32::to_bits) && reach.to_bits() == all[k].1.reach.to_bits() {
                idle += 1;
            } else {
                other_state += 1;
                state_of(&mine[all[k].0], l);
            }
        }
    }
    println!(
        "lamp objects' last kind-0 lights ({} objects, {} engine lights): {lights}, at an engine lamp's place bit for bit {found} (DIFFERENT -> {}), engine lights used {}; colour and reach those of the animation its graphic picks {idle}, on a stream (place only) {animated}, of another animation (the run sets the graphic: the replay's lamps, set_to_next.rs) {other_state}",
        last.len(),
        all.len(),
        lights - found,
        used.len()
    );
    println!("  the animations that give the other states: {states:?}");
}
