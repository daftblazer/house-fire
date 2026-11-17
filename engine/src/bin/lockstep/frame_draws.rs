//! The engine's draw list against the log's draws of one tick (LOCKSTEP_FRAME_DRAWS=TICK, stage
//! 5c): `drawlist` makes the list of the lot's model draws from the engine's world and the
//! disc's data alone; the log's side is the strips ('w') of the tick (a rigid strip, opcode 56, or
//! a display list, opcode 66) found in `models.arc` by their first bytes (`PartIndex::find`) and
//! the world matrix ('p' 0xF1) set up for each, as the renderer pairs them (`render/src/scene.rs`).
//!
//! Out of scope, counted apart: the Sims (skinned parts, their outfits' rigid parts, their
//! shadows), draws through another projection than the lot's (the portrait), draws whose model is
//! not in `models.arc`. Floors are the 'g' records, not strips: `scene::floor_runs`.
//!
//! A log draw and an engine draw are the same when they name the same part (a record whose
//! batch has the same positions: many wall records share bytes) and the world matrices agree.
//! Lines: `frame draws: log N, engine M`; `same`; `DIFFERENT (matched, matrix off)`, the log's
//! draw and an engine draw of the same part, nearest; `missing` (the log's, none left of its
//! part); `extra` (the engine's, left); each in total and by kind.

use crate::*;
use housefire::draw::Part;
use housefire::drawlist::{self, is_held, is_sims, Draw, Kind, Models};
use housefire::tree_log::{Log, StripRec};

/// A world matrix element is the same when it differs by less than this (the translation's tiles;
/// the 3 x 3 relative to its own scale).
const TOLERANCE: f32 = 1e-4;

use housefire::drawlist::VISIBLE;

/// Whether a part's place (`world`'s translation) is in `VISIBLE` of the logged view's screen.
fn on_screen(view: &[f32], projection: Option<[f32; 6]>, world: &[f32; 12]) -> bool {
    let Some(pr) = projection else { return true };
    let Ok(view): Result<[f32; 12], _> = view.get(..12).unwrap_or_default().try_into() else { return true };
    housefire::scene::Camera::on_screen(&view, &pr, [world[3], world[7], world[11]]).is_some_and(|s| (VISIBLE[0]..=VISIBLE[1]).contains(&s[0]) && (VISIBLE[2]..=VISIBLE[3]).contains(&s[1]))
}

/// A draw of the log's tick the engine's list answers for.
struct LogDraw<'a> {
    strip: usize,
    kind: Kind,
    candidates: Vec<Part<'a>>,
    world: [f32; 12],
}

fn kind_of(part: &Part<'_>, outer_lot: Option<usize>) -> Kind {
    if part.record.name.starts_with("wall_") {
        Kind::WallPiece
    } else if Some(part.index.0) == outer_lot {
        Kind::LotList
    } else if part.batch.list.is_some() {
        Kind::ObjectList
    } else {
        Kind::ObjectPart
    }
}

fn same_part(a: &Part<'_>, b: &Part<'_>) -> bool {
    a.index == b.index || (a.batch.count == b.batch.count && a.batch.list.is_some() == b.batch.list.is_some() && a.batch.positions == b.batch.positions)
}

/// The largest difference between two world matrices: the translation's tiles, and the 3 x 3
/// relative to the log's own scale.
fn off(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let scale = [0, 1, 2, 4, 5, 6, 8, 9, 10].iter().map(|&i| b[i].abs()).fold(0.0f32, f32::max).max(f32::MIN_POSITIVE);
    (0..12).map(|i| if i % 4 == 3 { (a[i] - b[i]).abs() } else { (a[i] - b[i]).abs() / scale }).fold(0.0, f32::max)
}

fn log_draws<'a>(log: &Log, models: &'a Models, tick: u32, outer_lot: Option<usize>, counts: &mut BTreeMap<&'static str, usize>) -> Vec<LogDraw<'a>> {
    let strips: Vec<usize> = (0..log.strips.len()).filter(|&i| log.strips[i].tick == tick).collect();
    let Some((&first, &last)) = strips.first().zip(strips.last()) else { return Vec::new() };
    // the records by the strips before them, as the renderer reads them
    let mut worlds: BTreeMap<usize, Vec<([f32; 12], u32)>> = BTreeMap::new();
    let mut projections: BTreeMap<usize, Vec<([f32; 6], u32)>> = BTreeMap::new();
    for m in &log.matrices {
        if m.projection {
            if m.id == 0 {
                projections.entry(m.strips_before).or_default().push((m.values[..6].try_into().unwrap(), m.tick));
            }
        } else if m.id == 0xF1 {
            worlds.entry(m.strips_before).or_default().push((m.values[..12].try_into().unwrap(), m.tick));
        }
    }
    let mut lights: BTreeMap<usize, &housefire::tree_log::LightsRec> = BTreeMap::new();
    for l in &log.lights {
        lights.insert(l.strips_before, l);
    }
    let (mut world, mut projection, mut main_projection, mut light) = (None, None, None, None);
    let mut out = Vec::new();
    for k in 0..=last {
        if let Some(l) = lights.get(&k).filter(|l| l.tick <= tick) {
            light = Some(*l);
        }
        if let Some(w) = worlds.get(&k) {
            world = w.last().map(|x| x.0);
        }
        if let Some(p) = projections.get(&k) {
            projection = p.last().map(|x| x.0);
        }
        if k < first {
            continue;
        }
        let strip: &StripRec = &log.strips[k];
        let op = strip.words[0] >> 24;
        if op != 0x38 && op != 0x42 {
            continue;
        }
        let use_world = worlds.get(&(k + 1)).and_then(|v| v.iter().find(|x| x.1 == tick)).map(|x| x.0).or(world);
        let use_projection = projections.get(&(k + 1)).and_then(|v| v.iter().find(|x| x.1 == tick)).map(|x| x.0).or(projection);
        main_projection = main_projection.or(projection).or(use_projection);
        let mut found = models.parts.find(strip);
        if op == 0x42 {
            found.retain(|p| p.batch.list.as_ref().is_some_and(|l| l.2.iter().zip(strip.at[6].iter()).all(|(a, b)| a == b)));
        }
        let lit_off = light.is_some_and(|l| l.flags() & 0x4000 != 0);
        let Some(part) = found.first().copied() else {
            *counts.entry("not in models.arc (skinned, run-time)").or_default() += 1;
            continue;
        };
        if use_projection != main_projection {
            *counts.entry("other projection (the portrait)").or_default() += 1;
        } else if is_held(&part.record.name) {
            *counts.entry("held by a Sim (its hand's bone)").or_default() += 1;
        } else if part.skinned() || lit_off || is_sims(&part.record.name) {
            *counts.entry("the Sims (skinned, outfits, shadows)").or_default() += 1;
            if std::env::var_os("LOCKSTEP_FRAME_SIMS").is_some() {
                println!("  sims: {} skinned {} lit-off {}", part.record.name, part.skinned(), lit_off);
            }
        } else if let Some(world) = use_world {
            out.push(LogDraw { strip: k, kind: kind_of(&part, outer_lot), candidates: found, world });
        } else {
            *counts.entry("no world matrix").or_default() += 1;
        }
    }
    out
}

pub fn frame_draws(log: &Log, world: &World, res: &BTreeMap<u32, housefire::quickdata::ObjResources>, d: &Disc, tick: u32) {
    let models = Models::read(d);
    let strip_ticks: BTreeSet<u32> = log.strips.iter().map(|s| s.tick).collect();
    println!("frame draws: the log's strips are at ticks {:?}", strip_ticks.iter().take(12).collect::<Vec<_>>());
    // the lot's own model: its house number's (the lot's global 10, which the game sets as it
    // enters the lot; the loaded lot keeps none yet: LOCKSTEP_FRAME_HOUSE, home 1) HouseData row
    let house = std::env::var("LOCKSTEP_FRAME_HOUSE").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
    let quick = d.read("quickdat.arc").unwrap_or_default();
    let houses = housefire::arc::entries(&quick).unwrap_or_default().into_iter().find(|e| e.name == "SimsUI").and_then(|e| housefire::quickdata::Image::read(housefire::arc::bytes(&quick, &e))).map(|img| housefire::quickdata::houses(&img)).unwrap_or_default();
    let outer_lot = houses.get(house).map(|h| h.outer_lot);
    let outer_record = outer_lot.and_then(|id| models.parts.record_by_id(id));

    let mut counts = BTreeMap::new();
    let theirs = log_draws(log, &models, tick, outer_record, &mut counts);

    // the engine's list, with the eye of the log's view of the tick (the camera is not the engine's own
    // yet), eye = -R^T t
    let view = log.matrices.iter().find(|m| !m.projection && m.id == 0xF0 && m.tick == tick).map(|m| m.values.clone()).unwrap_or_default();
    let projection: Option<[f32; 6]> = log.matrices.iter().find(|m| m.projection && m.id == 0 && m.tick == tick).and_then(|m| m.values.get(..6)?.try_into().ok());
    let eye = (view.len() >= 12).then(|| [0, 1].map(|i| -(0..3).map(|r| view[4 * r + i] * view[4 * r + 3]).sum::<f32>()));
    let lot = drawlist::lot_draws_of(world, res, &models, d, house, tick, eye);
    let (pieces, placements) = (lot.pieces, lot.placements);
    if std::env::var_os("LOCKSTEP_FRAME_WALLS").is_some() {
        // the log's wall draws by their place: the records every draw there could be, and each
        // draw's vertices and place in the first of them
        let mut groups: BTreeMap<(i64, i64, i64), Vec<&LogDraw<'_>>> = BTreeMap::new();
        for l in theirs.iter().filter(|l| l.kind == Kind::WallPiece) {
            let w = l.world;
            let angle = (w[4].atan2(w[0]).to_degrees() * 10.0).round() as i64;
            groups.entry(((w[3] * 100.0).round() as i64, (w[7] * 100.0).round() as i64, angle)).or_default().push(l);
        }
        for (k, v) in &groups {
            let mut names: Option<Vec<&str>> = None;
            for l in v {
                let n: Vec<&str> = l.candidates.iter().map(|c| c.record.name.as_str()).collect();
                names = Some(match names { None => n, Some(a) => a.into_iter().filter(|x| n.contains(x)).collect() });
            }
            let names = names.unwrap_or_default();
            let subs: Vec<String> = v.iter().map(|l| format!("{}@{}", l.candidates[0].batch.count, l.candidates.iter().find(|c| names.first().is_some_and(|n| c.record.name == *n)).map_or(99, |c| c.index.2 + 10 * c.index.1))).collect();
            let (px, py, pa) = (k.0 as f32 / 100.0, k.1 as f32 / 100.0, k.2 as f32 / 10.0);
            let piece = pieces.iter().find(|p| (p.x - px).abs() < 0.01 && (p.y - py).abs() < 0.01 && (p.angle - pa).abs() < 0.2 || (p.angle - pa + 360.0).rem_euclid(360.0) < 0.2 && (p.x - px).abs() < 0.01 && (p.y - py).abs() < 0.01);
            let (tx, ty) = (px.floor() as i32, py.floor() as i32);
            let diag: Vec<u8> = [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|(dx, dy)| world.walls.diagonal_bits(tx + dx, ty + dy)).collect();
            println!("  wall at ({}, {}) angle {}: {} in {:?}; piece {:?}; diagonals here,+x,-x,+y,-y {:?}", px, py, pa, subs.join(" "), names, piece.map(|p| (p.cap, p.opening, p.form)), diag);
        }
    }
    // the engine's list
    if std::env::var_os("LOCKSTEP_FRAME_DETAIL").is_some() {
        for p in &placements {
            let guids: Vec<u32> = world.find_object(p.leader).into_iter().chain(world.objects().filter(|o| o.leader == p.leader)).map(|o| o.guid).collect();
            let rs: Vec<String> = guids.iter().map(|g| match res.get(g) { None => format!("{g:08x}: no resources"), Some(r) => format!("{g:08x}: dataset {:08x} model {:08x} {} key {:08x} {} idle {:08x}; dataset models {:?}", r.dataset, r.model, models.parts.record_by_id(r.model).map_or("-".to_string(), |i| models.parts.record(i).name.clone()), r.key, models.parts.record_by_id(r.key).map_or("-".to_string(), |i| models.parts.record(i).name.clone()), r.idle, models.datasets.get(&r.dataset).map(|v| v.iter().map(|&i| models.parts.record_by_id(i).map_or("?".to_string(), |i| models.parts.record(i).name.clone())).collect::<Vec<_>>())) }).collect();
            println!("  object {} at ({}, {}): {}", p.leader, p.x, p.y, rs.join("; "));
        }
    }
    if let Ok(prefix) = std::env::var("LOCKSTEP_FRAME_RECORDS") {
        for r in 0.. {
            if r >= 100000 { break; }
            let Some(rec) = models.parts.try_record(r) else { break };
            if rec.name.starts_with(&prefix) {
                let subs: Vec<String> = rec.nodes.iter().map(|n| format!("bone {}: {}", n.bone, n.subs.iter().map(|(_, s)| format!("{}{}", s.count, if s.list.is_some() { "L" } else { "S" })).collect::<Vec<_>>().join(","))).collect();
                println!("record {} scale {:e}: {}", rec.name, rec.scale, subs.join(" | "));
                let all: Vec<&housefire::model::SubModel> = rec.nodes.iter().flat_map(|n| n.subs.iter().map(|s| &s.1)).collect();
                for (i, a) in all.iter().enumerate() {
                    println!("   sub {i}: count {} hash {:08x} first positions {:02x?} slots {:?}", a.count, a.hash, &a.positions[..8.min(a.positions.len())], a.slot_bytes);
                }
            }
        }
    }
    let mine: Vec<Draw> = lot.draws;

    // matching: per log draw, the engine draws of its part; the same when a world agrees
    let part_of = |d: &Draw| -> Part<'_> { models.parts.parts_of(d.part.0).find(|p| p.index == d.part).unwrap() };
    let mine_parts: Vec<Part<'_>> = mine.iter().map(part_of).collect();
    if let Ok(name) = std::env::var("LOCKSTEP_FRAME_MINE") {
        for (d, p) in mine.iter().zip(&mine_parts) {
            if p.record.name.starts_with(&name) {
                println!("  mine: {} node {} sub {} bone {:?} {} vertices: {:?}", p.record.name, p.index.1, p.index.2, p.bone(), p.batch.count, d.world.map(|v| format!("{v:.5e}")));
            }
        }
        for l in &theirs {
            if l.candidates[0].record.name.starts_with(&name) {
                println!("  log: {} {} vertices: {:?}", l.candidates[0].record.name, l.candidates[0].batch.count, l.world.map(|v| format!("{v:.5e}")));
            }
        }
    }
    let mut used = vec![false; mine.len()];
    let mut stat: BTreeMap<Kind, [usize; 7]> = BTreeMap::new(); // log, engine, same, different, missing, extra, extra off screen
    let mut shown = 0;
    // (LOCKSTEP_FRAME_DETAIL: the record names of the draws that are not the same, with counts)
    let mut by_name: BTreeMap<String, [usize; 3]> = BTreeMap::new();
    let mut later: Vec<&LogDraw<'_>> = Vec::new();
    for l in &theirs {
        stat.entry(l.kind).or_default()[0] += 1;
        let cands: Vec<usize> = (0..mine.len()).filter(|&i| !used[i] && l.candidates.iter().any(|c| same_part(c, &mine_parts[i]))).collect();
        if let Some(&i) = cands.iter().find(|&&i| off(&mine[i].world, &l.world) < TOLERANCE) {
            used[i] = true;
            stat.entry(l.kind).or_default()[2] += 1;
        } else {
            later.push(l);
        }
    }
    for l in later {
        let cands: Vec<usize> = (0..mine.len()).filter(|&i| !used[i] && l.candidates.iter().any(|c| same_part(c, &mine_parts[i]))).collect();
        let near = cands.iter().copied().min_by(|&a, &b| off(&mine[a].world, &l.world).total_cmp(&off(&mine[b].world, &l.world)));
        match near {
            Some(i) => {
                used[i] = true;
                by_name.entry(l.candidates[0].record.name.clone()).or_default()[0] += 1;
                stat.entry(l.kind).or_default()[3] += 1;
                if shown < 12 && l.candidates[0].record.name.starts_with(&std::env::var("LOCKSTEP_FRAME_SHOW").unwrap_or_default()) {
                    shown += 1;
                    println!("  matrix off by {:e}: strip {} {} mine {:?} the log's {:?}", off(&mine[i].world, &l.world), l.strip, l.candidates[0].record.name, mine[i].world, l.world);
                }
            }
            None => {
                by_name.entry(l.candidates[0].record.name.clone()).or_default()[1] += 1;
                stat.entry(l.kind).or_default()[4] += 1;
                if shown < 400 && l.kind.name().starts_with(&std::env::var("LOCKSTEP_FRAME_SHOW").unwrap_or_default()) {
                    shown += 1;
                    println!("  missing: strip {} {} {} vertices at ({}, {}, {}) angle {}", l.strip, l.kind.name(), l.candidates[0].batch.count, l.world[3], l.world[7], l.world[11], (l.world[1].atan2(l.world[0]).to_degrees() * 10.0).round() / 10.0);
                }
            }
        }
    }
    for (i, d) in mine.iter().enumerate() {
        stat.entry(d.kind).or_default()[1] += 1;
        if !used[i] {
            by_name.entry(mine_parts[i].record.name.clone()).or_default()[2] += 1;
            stat.entry(d.kind).or_default()[5] += 1;
            // whether the logged view's screen holds the part
            let seen = on_screen(&view, projection, &d.world);
            if !seen {
                stat.entry(d.kind).or_default()[6] += 1;
                by_name.entry(format!("{} (off screen)", mine_parts[i].record.name)).or_default()[2] += 1;
            }
            if seen && std::env::var_os("LOCKSTEP_FRAME_EXTRA").is_some() {
                let sc = housefire::scene::Camera::on_screen(&view[..12].try_into().unwrap(), &projection.unwrap(), [d.world[3], d.world[7], d.world[11]]);
                println!("  extra in view: {} {} vertices at ({}, {}, {}) screen {:?}", mine_parts[i].record.name, mine_parts[i].batch.count, d.world[3], d.world[7], d.world[11], sc);
            }
        }
    }
    if std::env::var_os("LOCKSTEP_FRAME_DETAIL").is_some() {
        let mut totals: BTreeMap<String, [usize; 2]> = BTreeMap::new();
        for l in &theirs {
            totals.entry(l.candidates[0].record.name.clone()).or_default()[0] += 1;
        }
        for p in &mine_parts {
            totals.entry(p.record.name.clone()).or_default()[1] += 1;
        }
        for (n, c) in totals.iter().filter(|(n, c)| c[0] != c[1] && !n.starts_with("wall_")) {
            println!("  totals {n}: log {}, engine {}", c[0], c[1]);
        }
        for (n, c) in &by_name {
            println!("  {n}: matrix off {}, missing {}, extra {}", c[0], c[1], c[2]);
        }
    }
    // the test on the log's own draws: any it would cull is a draw it cannot explain away
    let culled = theirs.iter().filter(|l| l.kind != Kind::LotList && !on_screen(&view, projection, &l.world)).count();
    for l in theirs.iter().filter(|l| l.kind != Kind::LotList && !on_screen(&view, projection, &l.world)) {
        println!("  the screen test culls: {} {} vertices at ({}, {}, {})", l.candidates[0].record.name, l.candidates[0].batch.count, l.world[3], l.world[7], l.world[11]);
    }
    println!("frame draws: the screen test would cull {culled} of the log's {} draws (none is the aim)", theirs.len());
    println!("frame draws, tick {tick}: set aside (out of scope): {counts:?}");
    let total = |j: usize| stat.values().map(|s| s[j]).sum::<usize>();
    let line = |label: &str, j: usize| {
        let by: Vec<String> = Kind::ALL.iter().map(|k| format!("{} {}", k.name(), stat.get(k).map_or(0, |s| s[j]))).collect();
        println!("frame draws: {label}: {} ({})", total(j), by.join(", "));
    };
    println!("frame draws: log {}, engine {}", total(0), total(1));
    line("same (model and world matrix within 1e-4)", 2);
    line("DIFFERENT (matched, matrix off)", 3);
    line("missing (log's, not in the engine's)", 4);
    line("extra (engine's, not drawn by the log)", 5);
    line("  of them off screen (camera culling)", 6);
}
