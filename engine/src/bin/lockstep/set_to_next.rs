//! SetToNext checks on a world loaded from the lot.

use crate::*;

/// SetToNext on a world loaded from the lot.
///
/// For each lot the log shows (a new lot: 20 or more objects never seen before ask for
/// variables in one tick), the disc's lot whose objects best match the ids seen then, loaded
/// natively; the objects that appear at the load and are not in the lot's table (two in the
/// club) go in front, not Sims. Each SetToNext step the original took on the stack object is
/// The room walk's lists and the original's room objects paired with them (a lockstep study).
type RoomLists = (Vec<Vec<(i32, i32)>>, std::collections::BTreeMap<u32, usize>);

#[allow(clippy::too_many_arguments)]
/// The disc's tables lockstep's checks use: the fence styles, the wall patterns, the floor
/// patterns (SimsTiles) and the objects' resources by GUID (SimsObjects).
pub type DiscTables = (Vec<u32>, Vec<u32>, Vec<housefire::quickdata::FloorPattern>, std::collections::BTreeMap<u32, housefire::quickdata::ObjResources>);

#[allow(clippy::too_many_arguments)]
pub fn set_to_next_check(lib: &Library, ngh: &[u8], disc_ngh: &[u8], tiles: &DiscTables, log: &housefire::tree_log::Log, step_object: &[Option<FileId>], engine_lamps: &[housefire::lamps::Lamp], notes: &mut Notes) {
    let (lots, lot_ids, walk, best, best_n) = pick_lot(lib, ngh, log);
    let raw_of = |s: &StepRec| -> Option<&[u8]> {
        let t = log.tree_of(s)?;
        if s.look != b'X' || t.tree != s.tree || s.node as usize * 12 >= t.raw.len() {
            return None;
        }
        Some(&t.raw[12 * s.node as usize..12 * s.node as usize + 12])
    };
    let mut lw = World::new(lib);
    lw.load_lot(&lots[best]).unwrap();
    // Objects made at load: in the walk but not in the lot, in front in the walk's order (when
    // the log has the original's list, it is replayed instead, below).
    let made: Vec<i16> = if log.lists.is_empty() { walk.iter().copied().filter(|id| !lot_ids[best].contains(id)).collect() } else { Vec::new() };
    for &id in made.iter().rev() {
        let mut o = Object::new();
        o.data.resize(DATA_OBJECT_ID + 1, 0);
        o.data[DATA_OBJECT_ID] = id;
        lw.add(o, true);
    }
    notes.note(&buf(format!("SetToNext: the disc's lot {best} (the walk of {} follows its order for {best_n})", walk.len()), 96));
    // The lot's saved state against the original: the first read of each of an object's own
    // variables (data, attributes, temps), before any expression wrote it, must be the lot's
    // value. Objects by their id (data 11, read as they ask).
    {
        let mut id_of: BTreeMap<u32, i16> = BTreeMap::new(); // the original's object pointer -> id
        let mut done: BTreeSet<(u32, i32, i32)> = BTreeSet::new();
        let diffs = std::env::var_os("LOCKSTEP_LOT_DIFFS").is_some();
        // the tick an id first leaves the original's list (after it, the id may be another
        // object's, not the lot's)
        let mut left: BTreeMap<i16, u32> = BTreeMap::new();
        for w in log.lists.windows(2) {
            for id in w[0].ids.iter().filter(|id| !w[1].ids.contains(id)) {
                left.entry(*id).or_insert(w[1].tick);
            }
        }
        for s in &log.steps {
            for v in &s.lookups {
                if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                    continue;
                }
                if v.scope == 3 && v.data as usize == DATA_OBJECT_ID {
                    id_of.insert(v.object, v.value);
                }
                if v.scope == 21 && left.get(&s.stack_object).is_some_and(|&t| s.tick > t) {
                    notes.note("scope 21: the stack object's id left the lot before (another object's now)");
                    continue;
                }
                if v.scope == 21 {
                    // The stack object's definition: field `datum` of its OBJD.
                    let Some(so) = lw.find(s.stack_object).filter(|&k| lw.get(k).definition.is_some()) else {
                        notes.note("scope 21: stack object or its definition not known");
                        continue;
                    };
                    let mut sf = Frame { stack_object: s.stack_object, ..Default::default() };
                    let got = ScriptVariables::new(&mut lw, so, &mut sf).get(21, v.data); // the engine's own lookup
                    let f = got.map_or(-32768, |g| g.value);
                    let e = buf(format!("datum {} of {}: native {f}, original {}", v.data, s.stack_object, v.value), 96);
                    notes.note_ex(if f == v.value { "scope 21: the definition's field the same" } else { "scope 21: DIFFERENT" }, &e);
                    if f != v.value {
                        let file = lw.get(so).file.map_or("?".to_string(), |f| {
                            let p = &lib.file(f).path;
                            p[p.rfind('/').map_or(0, |s| s + 1)..].to_string()
                        });
                        notes.note(&format!(
                            "  scope 21 differs: datum {} object {} ({file}) native {f} original {}",
                            v.data, s.stack_object, v.value
                        ));
                    }
                    continue;
                }
                if !matches!(v.scope, 3 | 0 | 8 | 14 | 18) {
                    continue;
                }
                if !done.insert((v.object, v.scope as i32, v.data as i32)) {
                    continue;
                }
                let Some(&id) = id_of.get(&v.object) else {
                    notes.note("lot state: object not known yet");
                    continue;
                };
                let Some(o) = lw.find_object(id) else {
                    notes.note("lot state: object not in the lot");
                    continue;
                };
                if v.scope == 14 {
                    // a Sim's motive: the float the lot saved
                    if o.motives.is_empty() {
                        notes.note("lot state: scope 14: not a Sim of the lot");
                        continue;
                    }
                    let Some(&m) = usize::try_from(v.data).ok().and_then(|k| o.motives.get(k)) else {
                        notes.note("lot state: scope 14: past the table");
                        continue;
                    };
                    let f = f32::from_bits(v.fvalue);
                    notes.note(&format!(
                        "lot state: scope 14{}",
                        if m == f { ": the first read the lot's value" } else { ": the first read not the lot's (changed before it)" }
                    ));
                    if m != f && diffs {
                        eprintln!("scope 14 tick {} object {id} datum {}: lot {}, read {}", s.tick, v.data, g(m as f64, 6), g(f as f64, 6));
                    }
                    continue;
                }
                let t: Option<&Vec<i16>> = match v.scope {
                    3 => Some(&o.data),
                    0 => Some(&o.attributes),
                    18 => Some(&o.person_data),
                    _ => None,
                };
                if v.scope == 18 && o.person_data.is_empty() {
                    notes.note("lot state: scope 18: not a Sim of the lot");
                    continue;
                }
                let native = match t {
                    Some(t) => match usize::try_from(v.data).ok().and_then(|k| t.get(k)) {
                        Some(&x) => x,
                        None => {
                            notes.note(&format!("lot state: scope {}: past the table", v.scope));
                            continue;
                        }
                    },
                    None => {
                        if !(0..8).contains(&v.data) {
                            continue;
                        }
                        o.temps[v.data as usize]
                    }
                };
                notes.note(&format!(
                    "lot state: scope {}{}",
                    v.scope,
                    if native == v.value { ": the first read the lot's value" } else { ": the first read not the lot's (changed before it)" }
                ));
                if native != v.value && v.scope == 18 && diffs {
                    eprintln!("scope 18 tick {} object {id} datum {}: lot {native}, read {}", s.tick, v.data, v.value);
                }
            }
            // A written variable's first read after it is not the lot's to check.
            if s.expr.is_some()
                && let Some(l) = s.lookups.last()
                && l.caller == EXPR_LHS
            {
                done.insert((l.object, l.scope as i32, l.data as i32));
            }
        }
    }
    // No object of the world's: SetToNext reads the frame alone (scope 10, the parameters).
    let nobody = ObjectKey(u32::MAX);
    // Data fields 2 (the container) and 59 of each object as the log reads them (scopes 3 and 4:
    // the address back to the object, the field at +0x26 + 2 x field), by step: what DropOnto and
    // SetToNext kind 6 read, set at run time by an object's own scripts, so a made object's are
    // taken from the log.
    const LOGGED_FIELDS: [i16; 2] = [DATA_CONTAINER as i16, 59];
    let mut fields: BTreeMap<(u32, i16), Vec<(usize, i16)>> = BTreeMap::new();
    for (i, s) in log.steps.iter().enumerate() {
        for v in s.lookups.iter().filter(|v| matches!(v.scope, 3 | 4) && LOGGED_FIELDS.contains(&v.data) && v.flags & 1 != 0) {
            let a = if v.ptr != 0 { v.ptr } else { v.fptr };
            let owner = if a != 0 { a.wrapping_sub(0x26 + 2 * v.data as u32) } else if v.scope == 3 { v.object } else { continue };
            fields.entry((owner, v.data)).or_default().push((i, v.value));
        }
        // an expression writing one: the value after the step, the owner from the left side's
        // lookup in the same step
        if let Some(e) = s.expr.as_ref().filter(|e| matches!(e.lhs_scope, 3 | 4) && LOGGED_FIELDS.contains(&e.lhs_data) && e.flags & 1 != 0)
            && let Some(v) = s.lookups.iter().find(|v| v.scope == e.lhs_scope && v.data == e.lhs_data && v.ptr != 0)
        {
            fields.entry((v.ptr.wrapping_sub(0x26 + 2 * e.lhs_data as u32), e.lhs_data)).or_default().push((i, e.lhs_after));
        }
    }
    // a made object's logged fields as they stood before step i
    let set_made_fields = |lw: &mut World, made_ptr: &BTreeMap<i16, u32>, i: usize| {
        for (&id, &ptr) in made_ptr {
            let Some(k) = lw.find(id) else { continue };
            for f in LOGGED_FIELDS {
                if let Some(&(_, v)) = fields.get(&(ptr, f)).and_then(|r| r.iter().take_while(|e| e.0 < i).last()) {
                    let d = &mut lw.get_mut(k).data;
                    if d.len() <= f as usize {
                        d.resize(f as usize + 1, 0);
                    }
                    d[f as usize] = v;
                }
            }
        }
    };
    let mut made_ptr: BTreeMap<i16, u32> = BTreeMap::new(); // a made object's id -> its pointer
    // Each stack's object (its sim -> the object whose own variables it reads most) and each
    // object pointer's id in the lists.
    let ptr_id: BTreeMap<u32, i16> = log.lists.iter().flat_map(|l| l.objects.iter().copied().zip(l.ids.iter().copied())).collect();
    let sim_object: BTreeMap<u32, u32> = {
        let mut own: BTreeMap<u32, BTreeMap<u32, usize>> = BTreeMap::new();
        for s in &log.steps {
            for v in s.lookups.iter().filter(|v| matches!(v.scope, 0 | 3 | 8)) {
                *own.entry(s.sim).or_default().entry(v.object).or_default() += 1;
            }
        }
        own.into_iter().filter_map(|(sim, c)| c.into_iter().max_by_key(|e| e.1).map(|e| (sim, e.0))).collect()
    };
    let mut li = 0;
    let mut shown = 0;
    let mut place_at = log.places.iter().position(|p| p.tick > 0).unwrap_or(log.places.len()); // the next 'P' after the lot's
    // study (LOCKSTEP_DUMP_PLACES=TICK, LOCKSTEP_DUMP_OUT=PATH): this world's objects as the
    // step loop reaches the tick (id, GUID, place in sixteenths, level, facing, container, file),
    // one a line, then its multi-tile leader (0: none), for the assets tool's check of the original's world matrices
    let dump_at = std::env::var("LOCKSTEP_DUMP_PLACES").ok().and_then(|v| v.parse::<u32>().ok());
    let mut dumped = false;
    // (LOCKSTEP_ROOM_AMBIENT=LOG,SNAPSHOT: each tick's inside room ambient factors from the
    // engine's world as the step loop reaches the tick: the walk's lists, the openings rule on
    // the objects then, `window_brightness`, `room_ambient` with the '5' record's clock and
    // day/night values; against every '6' brightness and '5' result of the LOG (the same
    // replay), its room objects paired with the engine's lists through the snapshot's)
    let ambient_study = std::env::var("LOCKSTEP_ROOM_AMBIENT").ok().and_then(|v| {
        let (a, b) = v.split_once(',')?;
        Some((read_log(a)?, std::fs::read(b).ok()?))
    });
    let mut ambient_lists: Option<RoomLists> = None;
    let (mut amb_k, mut amb_tick, mut amb_counts) = (0usize, u32::MAX, [0usize; 4]);
    let mut amb_shown = 0;
    for (i, s) in log.steps.iter().enumerate() {
        if let Some((alog, snap)) = &ambient_study
            && s.tick != amb_tick
        {
            amb_tick = s.tick;
            let (lists, by_room) = ambient_lists.get_or_insert_with(|| {
                let open = |x: i32, y: i32, dx: i32, dy: i32| !lw.walls.wall_into(x, y, dx, dy) || matches!(lw.walls.style(x, y, dx, dy), 2 | 12 | 13 | 14);
                let lists = housefire::light::room_tile_lists((39, 39), |x, y| lw.walls.diagonal_bits(x, y), open);
                let byte = |a: u32| a.checked_sub(0x8000_0000).and_then(|o| snap.get(o as usize).copied());
                let mut by_room = std::collections::BTreeMap::new();
                for w in &alog.window_brightness {
                    if let (Some(y), Some(x)) = (byte(w[2]), byte(w[2] + 1))
                        && let Some(k) = lists.iter().position(|l| l.first() == Some(&(i32::from(x), i32::from(y))))
                    {
                        by_room.insert(w[1], k);
                    }
                }
                (lists, by_room)
            });
            while amb_k < alog.window_brightness.len() && alog.window_brightness[amb_k][0] <= s.tick {
                let w = alog.window_brightness[amb_k];
                if w[0] == s.tick
                    && let Some(&k) = by_room.get(&w[1])
                {
                    let m = &lists[k];
                    let halves: i32 = m.iter().map(|&(x, y)| if lw.walls.diagonal_bits(x, y) != 0 { 1 } else { 2 }).sum();
                    // (the persons where the log's places put them before this tick: lockstep's
                    // world does not walk them)
                    let person_at = |id: i16| alog.places.iter().rev().find(|p| p.id == id && p.tick < s.tick).map(|p| (p.x >> 4, p.y >> 4, p.level));
                    let openings = m
                        .iter()
                        .filter(|&&(x, y)| {
                            housefire::light::tile_counts_as_opening(
                                lw.objects()
                                    .filter(|o| o.on_lot() && if o.person { person_at(o.id()) == Some((x, y, 1)) } else { o.level == 1 && (o.x16 >> 4, o.y16 >> 4) == (x, y) })
                                    .map(|o| (o.door || o.window, o.person, o.data.get(housefire::world::DATA_CONTAINER).copied().unwrap_or(0) != 0)),
                            )
                        })
                        .count() as u32;
                    let b = housefire::light::window_brightness(openings, halves / 2);
                    amb_counts[0] += 1;
                    if b.to_bits() == w[4] {
                        amb_counts[1] += 1;
                    } else if amb_shown < 5 {
                        amb_shown += 1;
                        println!("room ambient: tick {} room {:08x}: brightness {b}, the log's {}", w[0], w[1], f32::from_bits(w[4]));
                    }
                    if let Some(a) = alog.room_ambients.get(amb_k) {
                        let f = f32::from_bits;
                        let mine = housefire::light::room_ambient(f(a[4]), f(a[5]), a[6] as i32, a[7] as i32, a[8] as i32, b);
                        amb_counts[2] += 1;
                        if mine.to_bits() == a[9] {
                            amb_counts[3] += 1;
                        }
                    }
                }
                amb_k += 1;
            }
        }
        if ambient_study.is_some() && i + 1 == log.steps.len() {
            println!("room ambient: brightness from the engine {} of {}; ambient factors {} of {}", amb_counts[1], amb_counts[0], amb_counts[3], amb_counts[2]);
        }
        if let Some(t) = dump_at
            && !dumped
            && s.tick >= t
        {
            dumped = true;
            let mut out = String::new();
            for o in lw.objects() {
                let path = o.file.map_or("", |f| lib.file(f).path.as_str());
                out += &format!(
                    "{} {:08x} {} {} {} {} {} {} {} {}\n",
                    o.id(),
                    o.guid,
                    o.x16,
                    o.y16,
                    o.level,
                    o.data.get(DATA_FACING).copied().unwrap_or(0),
                    o.data.get(DATA_CONTAINER).copied().unwrap_or(0),
                    u8::from(o.person),
                    path,
                    o.leader
                );
            }
            let at = std::env::var("LOCKSTEP_DUMP_OUT").unwrap_or_else(|_| "work/logs/places.txt".into());
            if std::fs::write(&at, out).is_ok() {
                println!("places at tick {} written to {at}", s.tick);
            }
            // and the walls' pieces as the engine draws them (LOCKSTEP_DUMP_WALLS: x y angle
            // fence pattern cap opening form), a lot whose save keeps no walls (the club) with its house's, the disc's
            // copy's (as replay builds them; lockstep's own world untouched)
            if let Ok(at) = std::env::var("LOCKSTEP_DUMP_WALLS") {
                let house = (lw.walls.wall_edges() == 0)
                    .then(|| read_lots(disc_ngh).ok().and_then(|l| l.get(best).and_then(|h| housefire::save::read_lot_arrays(h).ok())))
                    .flatten()
                    .map(|a| housefire::routing::Walls::new(&a, &lw));
                let walls = house.as_ref().unwrap_or(&lw.walls);
                let mut out = String::new();
                // (a wall part's pattern as its shader's name hash, a fence's style)
                for p in housefire::scene::wall_pieces(walls, 64, &tiles.0) {
                    let shader = if p.fence { u32::from(p.pattern) } else { tiles.1.get(usize::from(p.pattern)).copied().unwrap_or(0) };
                    let opening = match p.opening {
                        housefire::routing::Opening::None => "plain",
                        housefire::routing::Opening::Door => "door",
                        housefire::routing::Opening::Window => "window",
                    };
                    let form = match p.form.suffix() {
                        "" => "straight",
                        s => s.trim_start_matches('_'),
                    };
                    out += &format!("{} {} {} {} {shader:08x} {} {opening} {form}\n", p.x, p.y, p.angle, u8::from(p.fence), u8::from(p.cap));
                }
                let _ = std::fs::write(&at, out);
                // (and each tile's floor, LOCKSTEP_DUMP_FLOORS: x y value shader, the non-zero
                // ones; the shader FloorSet's, 0 for a value past its list)
                if let Ok(at) = std::env::var("LOCKSTEP_DUMP_FLOORS") {
                    let mut out = String::new();
                    for y in 0..64 {
                        for x in 0..64 {
                            let v = walls.floor(x, y);
                            if v != 0 {
                                let shader = tiles.2.get(usize::from(v)).map_or(0, |p| p.shader);
                                out += &format!("{x} {y} {v} {shader:08x} {:x}\n", walls.diagonal_bits(x, y));
                            }
                        }
                    }
                    let _ = std::fs::write(&at, out);
                }
                // (LOCKSTEP_ROOM_LISTS=LOG[,SNAPSHOT]: the engine's room walk, `light::room_tile_lists`,
                // against the original's lists: the log's '7' records, or the '6' records' ranges
                // read from the snapshot; matched by their first tile, entry for entry)
                if let Ok(v) = std::env::var("LOCKSTEP_ROOM_LISTS") {
                    let mut parts = v.split(',');
                    let log = parts.next().and_then(read_log).expect("LOCKSTEP_ROOM_LISTS: the log");
                    let snap = parts.next().and_then(|p| std::fs::read(p).ok());
                    let mut theirs: Vec<Vec<(i32, i32)>> = log.room_tiles.iter().map(|r| r.3.iter().map(|e| (((e >> 8) & 0xff) as i32, (e >> 16) as i32)).collect()).collect();
                    let mut ptrs: Vec<u32> = log.room_tiles.iter().map(|r| r.1).collect();
                    if let Some(m) = &snap {
                        let byte = |a: u32| a.checked_sub(0x8000_0000).and_then(|o| m.get(o as usize).copied());
                        let mut seen = std::collections::BTreeSet::new();
                        for w in &log.window_brightness {
                            if seen.insert(w[1]) {
                                ptrs.push(w[1]);
                                theirs.push((w[2]..w[3]).step_by(3).filter_map(|a| Some((i32::from(byte(a + 1)?), i32::from(byte(a)?)))).collect());
                            }
                        }
                    }
                    let open = |x: i32, y: i32, dx: i32, dy: i32| !lw.walls.wall_into(x, y, dx, dy) || matches!(lw.walls.style(x, y, dx, dy), 2 | 12 | 13 | 14);
                    let mine = housefire::light::room_tile_lists((39, 39), |x, y| lw.walls.diagonal_bits(x, y), open);
                    println!("room lists: the engine's {}, the original's {}", mine.len(), theirs.len());
                    // (and each room's window brightness from the engine alone, against the '6'
                    // records of LOCKSTEP_ROOM_BRIGHTNESS=LOG nearest this tick: the openings rule on
                    // each entry, over the half count (2 a whole tile's entry, 1 a split tile's) / 2)
                    if let Some(blog) = std::env::var("LOCKSTEP_ROOM_BRIGHTNESS").ok().and_then(|p| read_log(&p)) {
                        for (t, ptr) in theirs.iter().zip(&ptrs) {
                            let Some(m) = t.first().and_then(|f| mine.iter().find(|m| m.first() == Some(f))) else { continue };
                            let halves: i32 = m.iter().map(|&(x, y)| if lw.walls.diagonal_bits(x, y) != 0 { 1 } else { 2 }).sum();
                            let openings = m
                                .iter()
                                .filter(|&&(x, y)| housefire::light::tile_counts_as_opening(lw.objects().filter(|w| w.on_lot() && w.level == 1 && (w.x16 >> 4, w.y16 >> 4) == (x, y)).map(|w| (w.door || w.window, w.person, w.data.get(housefire::world::DATA_CONTAINER).copied().unwrap_or(0) != 0))))
                                .count() as u32;
                            let b = housefire::light::window_brightness(openings, halves / 2);
                            let logged = blog.window_brightness.iter().filter(|w| w[1] == *ptr).min_by_key(|w| (i64::from(w[0]) - i64::from(s.tick)).abs());
                            let half_logged = blog.window_entries.iter().zip(&blog.window_brightness).find(|(_, w)| w[1] == *ptr).map(|(e, _)| e.0);
                            println!("  room {ptr:08x}: halves {halves} (the log's +72 {half_logged:?}), openings {openings}, brightness {b} {}", match logged {
                                Some(w) if w[4] == b.to_bits() => format!("as the log's at tick {}", w[0]),
                                Some(w) => format!("DIFFERENT: the log's {} at tick {}", f32::from_bits(w[4]), w[0]),
                                None => "(no record)".into(),
                            });
                        }
                    }
                    for t in &theirs {
                        let Some(m) = t.first().and_then(|f| mine.iter().find(|m| m.first() == Some(f))) else {
                            println!("  from {:?} ({} entries): no engine room starts there", t.first(), t.len());
                            continue;
                        };
                        let first_diff = t.iter().zip(m).position(|(a, b)| a != b);
                        if let Some(k) = first_diff.filter(|_| std::env::var_os("LOCKSTEP_ROOM_LISTS_AROUND").is_some()) {
                            let lo = k.saturating_sub(12);
                            println!("    the original's {lo}..: {:?}", &t[lo..(k + 6).min(t.len())]);
                            println!("    the engine's   {lo}..: {:?}", &m[lo..(k + 6).min(m.len())]);
                            for &(x, y) in &t[lo..(k + 6).min(t.len())] {
                                print!(" ({x},{y}): d{:#x} w{:?}", lw.walls.diagonal_bits(x, y), [(-1, 0), (1, 0), (0, -1), (0, 1)].map(|(dx, dy)| (lw.walls.wall_between(x, y, dx, dy), lw.walls.pattern(x, y, dx, dy))));
                            }
                            println!();
                        }
                        println!("  from {:?}: the original's {} entries, the engine's {}{}", t.first(), t.len(), m.len(), match first_diff {
                            None if t.len() == m.len() => " (the same)".to_string(),
                            None => " (the same as far as the shorter)".to_string(),
                            Some(k) => format!(", first different at {k}: {:?} against {:?}", &t[k..(k + 4).min(t.len())], &m[k..(k + 4).min(m.len())]),
                        });
                    }
                }
                // (study, LOCKSTEP_ROOM_WINDOWS=LOG,SNAPSHOT: each room's tile list from the
                // snapshot (the '6' records' ranges of (x, y, level) bytes), its tile count (+72 / 2),
                // the engine's window objects on those tiles, against the window share the log's
                // brightness gives)
                if let Some((lp, sp)) = std::env::var("LOCKSTEP_ROOM_WINDOWS").ok().and_then(|v| v.split_once(',').map(|(a, b)| (a.to_string(), b.to_string()))) {
                    let (Some(log), Ok(m)) = (read_log(&lp), std::fs::read(&sp)) else { panic!("LOCKSTEP_ROOM_WINDOWS: unreadable") };
                    let byte = |a: u32| a.checked_sub(0x8000_0000).and_then(|o| m.get(o as usize).copied());
                    let word = |a: u32| (0..4).try_fold(0u32, |w, k| Some(w << 8 | u32::from(byte(a + k)?)));
                    let mut rooms = std::collections::BTreeMap::new();
                    for w in &log.window_brightness {
                        rooms.insert(w[1], (w[2], w[3], w[4]));
                    }
                    for (o, (b, e, r)) in rooms {
                        let tiles: Vec<(i32, i32, i32)> = (b..e).step_by(3).filter_map(|a| Some((i32::from(byte(a)?), i32::from(byte(a + 1)?), i32::from(byte(a + 2)?)))).collect();
                        let count = word(o + 72).map_or(0, |v| (v as i32) / 2);
                        let implied = (f32::from_bits(r) - 1.0) / 0.25 * count as f32;
                        let windows: Vec<String> = lw.objects().filter(|w| w.window && w.on_lot()).filter(|w| tiles.contains(&(w.x16 >> 4, w.y16 >> 4, w.level))).map(|w| format!("{}@{},{},{} f{} lead{}", w.id(), w.x16 >> 4, w.y16 >> 4, w.level, w.facing(), w.leader == w.id())).collect();
                        let all: Vec<String> = lw.objects().filter(|w| w.window && w.on_lot()).map(|w| format!("{},{},{}", w.x16 >> 4, w.y16 >> 4, w.level)).collect();
                        println!("room {o:08x}: {} tile entries, count {count}, implied windows {implied}; the engine's window objects on them {}: {windows:?}", tiles.len(), windows.len());
                        let _ = all;
                        // every object on the room's tile entries, counted per entry (as the loop goes)
                        let mut per: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
                        for t in &tiles {
                            for w in lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == *t) {
                                *per.entry(w.file.map_or("?".into(), |f| lib.file(f).path.clone())).or_default() += 1;
                            }
                        }
                        println!("  objects per entry {per:?}");
                        // the entries in the engine's axes with the engine's room of each
                        println!("  entries (x, y: room) {:?}", tiles.iter().map(|t| (t.1, t.0, lw.walls.room(t.1, t.0))).collect::<Vec<_>>());
                        // the definition fields that are 8 for some object on the entry, counted per entry
                        let mut by_field: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
                        for t in &tiles {
                            let mut fields = std::collections::BTreeSet::new();
                            for w in lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == *t) {
                                if let Some(d) = lw.definition(w) {
                                    for (k, v) in d.fields.iter().enumerate() {
                                        if *v == 8 {
                                            fields.insert(k);
                                        }
                                    }
                                }
                            }
                            for k in fields {
                                *by_field.entry(k).or_default() += 1;
                            }
                        }
                        println!("  entries with a field at 8, by field {by_field:?}");
                        // (logs with the entries: each kind 8 entry's tile against its opening's
                        // own tile and facing, by the log's id)
                        if let Some(k) = log.window_brightness.iter().position(|w| w[1] == o) {
                            let list = log.lists.iter().rev().find(|l| l.tick <= log.window_brightness[k][0]);
                            let mut per: std::collections::BTreeMap<i16, Vec<(i32, i32)>> = std::collections::BTreeMap::new();
                            for (t, e) in tiles.iter().zip(&log.window_entries[k].1) {
                                if e[2] as i32 != 8 {
                                    continue;
                                }
                                if let Some(id) = list.and_then(|l| l.objects.iter().position(|p| *p == e[1].wrapping_add(112)).map(|j| l.ids[j])) {
                                    per.entry(id).or_default().push((t.0, t.1));
                                }
                            }
                            // the engine's: entries (a, b) with a door or window standing at (b, a)
                            // (the entries' bytes are (y, x, level) in the engine's axes)
                            let counted = |t: &(i32, i32, i32)| {
                                housefire::light::tile_counts_as_opening(
                                    lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == (t.1, t.0, t.2)).map(|w| (w.door || w.window, w.person, w.data.get(housefire::world::DATA_CONTAINER).copied().unwrap_or(0) != 0)),
                                )
                            };
                            let mine = tiles.iter().filter(|t| counted(t)).count();
                            let theirs = log.window_entries[k].1.iter().filter(|e| e[2] as i32 == 8).count();
                            println!("    openings: the engine's {mine}, the log's {theirs}");
                            for (t, e) in tiles.iter().zip(&log.window_entries[k].1) {
                                let here: Vec<String> = lw.objects().filter(|w| w.on_lot() && (w.x16 >> 4, w.y16 >> 4, w.level) == (t.1, t.0, t.2)).map(|w| format!("{}{}:{}", w.id(), if w.door || w.window { "*" } else { "" }, w.file.map_or("", |f| lib.file(f).path.rsplit('/').next().unwrap_or("")))).collect();
                                let opening = counted(t);
                                if opening != (e[2] as i32 == 8) {
                                    let id = list.and_then(|l| l.objects.iter().position(|p| *p == e[1].wrapping_add(112)).map(|j| l.ids[j]));
                                    println!("      tile {t:?}: the log's kind {} id {id:?} grid {}; the engine's objects there {here:?}", e[2] as i32, e[0] as i32);
                                }
                            }
                            for (id, ts) in per {
                                let me = lw.find_object(id).map(|w| (w.x16 >> 4, w.y16 >> 4, w.facing(), w.leader, w.file.map_or(String::new(), |f| lib.file(f).path.rsplit('/').next().unwrap_or("").to_string())));
                                println!("    opening {id} {me:?}: its entries' tiles {ts:?}");
                            }
                        }
                    }
                }
                // (and the windows' lights, LOCKSTEP_WINDOW_LIGHTS=LOG: the engine's places for each
                // window's leading object against the other lights of the log's last 't' record,
                // x and y bit for bit, as sets)
                if let Some(log) = std::env::var("LOCKSTEP_WINDOW_LIGHTS").ok().and_then(|p| read_log(&p)) {
                    let mut mine: Vec<(u32, u32)> = lw
                        .objects()
                        .filter(|o| o.window && o.on_lot() && o.leader == o.id())
                        .filter_map(|o| housefire::light::window_light_places((o.x16 >> 4, o.y16 >> 4), o.facing() as u8))
                        .flatten()
                        .map(|(x, y)| (x.to_bits(), y.to_bits()))
                        .collect();
                    let mut theirs: Vec<(u32, u32)> = log.light_rebuilds.iter().rev().find(|b| b.rooms > 0).map_or(Vec::new(), |b| b.lights[..b.other as usize].iter().map(|l| (l[0], l[1])).collect());
                    if std::env::var_os("LOCKSTEP_WINDOW_FIELDS").is_some() {
                        let mut seen = std::collections::BTreeSet::new();
                        for o in lw.objects().filter(|o| o.on_lot() && o.leader == o.id() && (o.window || o.file.is_some_and(|f| lib.file(f).path.contains("window")))) {
                            let path = o.file.map_or("", |f| lib.file(f).path.as_str());
                            if seen.insert(path.to_string()) {
                                println!("  {path} window {}: fields {:?}", o.window, lw.definition(o).map(|d| d.fields.clone()));
                                println!("    data {:?}", o.data);
                                println!("    attributes {:?}", o.attributes);
                            }
                        }
                    }
                    mine.sort_unstable();
                    theirs.sort_unstable();
                    let same = mine.iter().filter(|p| theirs.contains(p)).count();
                    println!("window lights: the engine's {}, the log's {}, places the same {same}{}", mine.len(), theirs.len(), if mine == theirs { " (all)" } else { "" });
                    // (each window's kind from its model, against the log's 'y' kind of the light
                    // at the engine's place)
                    if let Some(b) = log.light_rebuilds.iter().rev().find(|b| b.rooms > 0) {
                        let (mut n, mut same_kind) = (0, 0);
                        // (and each light's nearest lights: the room of the tile under the window's
                        // other light (the wall's other side) less one, ranked over that room's own range at
                        // the grid build before each fill, against the fill's list)
                        let (mut fills, mut near_same, mut first_fills) = (0, 0, 0);
                        for o in lw.objects().filter(|o| o.window && o.on_lot() && o.leader == o.id()) {
                            let Some(places) = housefire::light::window_light_places((o.x16 >> 4, o.y16 >> 4), o.facing() as u8) else { continue };
                            // (the group's model: a multi-tile object's resources are on one of its
                            // tiles' definitions, not the leader's)
                            let model = lw.objects().filter(|p| p.leader == o.id()).find_map(|p| tiles.3.get(&p.guid)).map(|r| r.key);
                            let kind = model.map_or(0, housefire::light::window_kind);
                            if std::env::var_os("LOCKSTEP_WINDOW_FIELDS").is_some() {
                                println!("  window {} guid {:08x}: key {model:08x?}", o.id(), o.guid);
                            }
                            for (side, (x, y)) in places.into_iter().enumerate() {
                                let (ox, oy) = places[1 - side];
                                let room = walls.room(ox.floor() as i32, oy.floor() as i32).wrapping_sub(1) as usize;
                                let Some(index) = b.lights.iter().position(|l| l[0] == x.to_bits() && l[1] == y.to_bits()) else { continue };
                                let light = &b.lights[index];
                                for w in log.window_fills.iter().filter(|w| w.light == index as u32) {
                                    let Some(g) = log.grid_builds.iter().rev().find(|g| g.tick < w.tick) else {
                                        first_fills += 1;
                                        continue;
                                    };
                                    fills += 1;
                                    let Some(r) = g.ranges.get(room) else { continue };
                                    let mine: Vec<u32> = housefire::light::nearest_lights(w.at, (r[2], r[3]), &g.lights).map_or(Vec::new(), |m| m.iter().map_while(|s| s.map(u32::from)).collect());
                                    if mine == w.near.iter().map(|x| x.0).collect::<Vec<_>>() {
                                        near_same += 1;
                                    } else if std::env::var_os("LOCKSTEP_NEAREST_SHOW").is_some() && w.tick < 3 {
                                        let fits: Vec<usize> = (0..16).filter(|&k| housefire::light::nearest_lights(w.at, (g.ranges[k][2], g.ranges[k][3]), &g.lights).is_some_and(|m| m.iter().map_while(|s| s.map(u32::from)).collect::<Vec<_>>() == w.near.iter().map(|x| x.0).collect::<Vec<_>>())).collect();
                                        let (tx, ty) = (o.x16 >> 4, o.y16 >> 4);
                                        println!("  light {index} window {} at ({tx},{ty}) facing {}: engine room {room}, fits {fits:?}; rooms around {:?}", o.id(), o.facing(), [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dy)| walls.room(tx + dx, ty + dy).wrapping_sub(1) as i32));
                                    }
                                }
                                let Some((h, _)) = log.window_lights.iter().find(|(h, _)| h[0] == light[7]) else { continue };
                                n += 1;
                                if h[6] == kind {
                                    same_kind += 1;
                                }
                            }
                        }
                        println!("window lights' kinds: {n} found, the engine's (by its resources' key) {same_kind}");
                        // (and the other lights' array whole: each window in list order, its two
                        // lights (front, behind), each in the room under its own place and facing
                        // the other's; sorted room by room (`sort_lights`); its x, y, height and
                        // reach against the rebuild's lights in order)
                        let mut objs = Vec::new();
                        let mut info = Vec::new();
                        for o in lw.objects().filter(|o| o.window && o.on_lot() && o.leader == o.id()) {
                            let Some(places) = housefire::light::window_light_places((o.x16 >> 4, o.y16 >> 4), o.facing() as u8) else { continue };
                            let model = lw.objects().filter(|p| p.leader == o.id()).find_map(|p| tiles.3.get(&p.guid)).map(|r| r.key);
                            let kind = model.map_or(0, housefire::light::window_kind);
                            let room_at = |(x, y): (f32, f32)| i32::from(walls.room(x.floor() as i32, y.floor() as i32).wrapping_sub(1));
                            for side in 0..2 {
                                let (me, other) = (room_at(places[side]), room_at(places[1 - side]));
                                objs.push((housefire::light::LightKind::Window, me, other));
                                info.push((places[side], kind, me));
                            }
                        }
                        let rooms = b.rooms as usize;
                        let (_, order, _) = housefire::light::sort_lights(rooms, &objs);
                        let theirs = &b.lights[..b.other as usize];
                        let (mut placed, mut heights, mut reaches) = (0, 0, 0);
                        for (k, &i) in order.iter().enumerate() {
                            let ((x, y), kind, me) = info[i];
                            let Some(t) = theirs.get(k) else { break };
                            if (t[0], t[1]) == (x.to_bits(), y.to_bits()) {
                                placed += 1;
                            }
                            if let Some((reach, _, h)) = housefire::light::window_light_params(kind, me != 0) {
                                if h.map(f32::to_bits) == Some(t[2]) {
                                    heights += 1;
                                }
                                if reach.to_bits() == t[3] {
                                    reaches += 1;
                                } else if std::env::var_os("LOCKSTEP_NEAREST_SHOW").is_some() {
                                    println!("  other light {k}: reach {reach}, the log's {} (kind {kind}, room {me})", f32::from_bits(t[3]));
                                }
                            }
                        }
                        println!("other lights' array: the engine's {}, the log's {}; in order the same place {placed}, height {heights}, reach {reaches}", order.len(), theirs.len());
                        // (and the grid build after this rebuild from the engine: its cells' rooms
                        // (`cell_room_adds` through `CellRooms`), the other ranges and lights (the
                        // array above), the own ranges and lights still the log's (the lamps'
                        // places wait for the poses); each cell room's four lights by `cell_lights`)
                        let (ranges, _, _) = housefire::light::sort_lights(rooms, &objs);
                        if let Some(g) = b.builds_before.checked_sub(1).and_then(|i| log.grid_builds.get(i)) {
                            let mut cells = housefire::light::CellRooms::default();
                            for (x, y, r) in housefire::light::cell_room_adds((walls.size().0.min(39), walls.size().1.min(39)), |x, y| walls.room(x, y).wrapping_sub(1) as u8, |x, y| walls.diagonal_bits(x, y)) {
                                cells.add(x, y, r);
                            }
                            let mut lights = g.lights.clone();
                            for (k, &i) in order.iter().enumerate() {
                                let ((x, y), kind, me) = info[i];
                                if let (Some(l), Some((reach, _, Some(h)))) = (lights.get_mut(k), housefire::light::window_light_params(kind, me != 0)) {
                                    *l = ([x, y, h], reach);
                                }
                            }
                            let (mut cell_rooms, mut rooms_same, mut lit_same) = (0, 0, 0);
                            for (ci, cell) in g.cells.iter().enumerate() {
                                let (x, y) = (2 + ci / 36, 2 + ci % 36);
                                let mine = cells.at(x as i32, y as i32).unwrap_or([0xFF; 4]);
                                let ids = cell[0].to_be_bytes();
                                if mine == ids {
                                    rooms_same += 1;
                                }
                                for (k, &id) in mine.iter().enumerate() {
                                    if id == 0xFF {
                                        break;
                                    }
                                    cell_rooms += 1;
                                    let logged: [u16; 4] = {
                                        let (a, c) = (cell[1 + 2 * k], cell[2 + 2 * k]);
                                        [(a >> 16) as u16, a as u16, (c >> 16) as u16, c as u16]
                                    };
                                    let (Some(other), Some(own)) = (ranges.get(usize::from(id)), g.ranges.get(usize::from(id))) else { continue };
                                    let point = [x as f32, y as f32, housefire::light::GRID_Z];
                                    if housefire::light::cell_lights(point, (own[2], own[3]), (other[0], other[1]), &lights) == Some(logged) {
                                        lit_same += 1;
                                    }
                                }
                            }
                            println!("grid build at tick {} from the engine: cells {}, rooms the same {rooms_same}; cell rooms {cell_rooms}, the four lights the same {lit_same}", g.tick, g.cells.len());
                        }
                        lamps_from_the_engine(&log, b, rooms, &objs, &info, &order, engine_lamps, &lw, walls);
                        println!("window lights' nearest lights: {fills} fills after a build ({first_fills} before any), from the other side's room {near_same}");
                    }
                }
                // (and the rooms the light grid is given at the load, LOCKSTEP_CELL_ROOMS=LOG: the
                // engine's adds against the log's 'f' records, add for add)
                if let Some(log) = std::env::var("LOCKSTEP_CELL_ROOMS").ok().and_then(|p| read_log(&p)) {
                    let mine = housefire::light::cell_room_adds((walls.size().0.min(39), walls.size().1.min(39)), |x, y| walls.room(x, y).wrapping_sub(1) as u8, |x, y| walls.diagonal_bits(x, y));
                    let theirs: Vec<(i32, i32, u8)> = log.cell_adds.iter().map(|a| (a[1] as i32, a[2] as i32, a[3] as u8)).collect();
                    let same = mine.iter().zip(&theirs).filter(|(a, b)| a == b).count();
                    let first = mine.iter().zip(&theirs).position(|(a, b)| a != b);
                    println!("light grid rooms: the engine's adds {}, the log's {}, the same in order {same} (first differing {first:?})", mine.len(), theirs.len());
                }
                // (and each tile's room, LOCKSTEP_DUMP_ROOMS: x y room diagonal bits, the whole
                // grid; the engine's rooms, 0 on a diagonal's tile and off the grid)
                if let Ok(at) = std::env::var("LOCKSTEP_DUMP_ROOMS") {
                    let mut out = String::new();
                    for y in 0..64 {
                        for x in 0..64 {
                            out += &format!("{x} {y} {} {:x}\n", walls.room(x, y), walls.diagonal_bits(x, y));
                        }
                    }
                    let _ = std::fs::write(&at, out);
                }
                // (and the floor runs the engine draws, LOCKSTEP_DUMP_RUNS: x y n shader lead trail
                // value: the whole tiles, the half tiles' diagonal bits)
                if let Ok(at) = std::env::var("LOCKSTEP_DUMP_RUNS") {
                    let mut out = String::new();
                    for r in housefire::scene::floor_runs(walls, 64, &tiles.2) {
                        out += &format!("{} {} {} {:08x} {:x} {:x} {}\n", r.x, r.y, r.n, tiles.2[usize::from(r.value)].shader, r.lead, r.trail, r.value);
                    }
                    let _ = std::fs::write(&at, out);
                }
            }
        }
        // The places the log has for the ticks before this step's (each the end of its tick),
        // against this world's for objects that are not persons: made, grabbed, dropped and
        // snapped natively (persons walk: routing's, not this world's)
        while place_at < log.places.len() && log.places[place_at].tick < s.tick {
            let p = log.places[place_at];
            place_at += 1;
            let Some(o) = lw.find_object(p.id).filter(|o| !o.person && !lw.find_object(p.id).is_some_and(|o| o.file.is_some_and(|f| lib.file(f).path.contains("template_person")))) else { continue };
            // a held object (container field an object) is at its holder's place: the holder's
            // as the log has it that tick (a person's place is routing's)
            let holder = o.data.get(DATA_CONTAINER).copied().filter(|&h| {
                h > 0 && h != p.id && lw.find_object(h).is_some_and(|c| c.person || c.file.is_some_and(|f| lib.file(f).path.contains("template_person")))
            });
            if let Some(h) = holder {
                let at = log.places.iter().rfind(|q| q.id == h && q.tick <= p.tick).map(|q| (q.x, q.y, q.level));
                notes.note(if at == Some((p.x, p.y, p.level)) {
                    "objects: a held object's place at a tick's end its holder's (the original's)"
                } else {
                    "objects: a held object's place at a tick's end NOT its holder's"
                });
                if at != Some((p.x, p.y, p.level)) && std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                    println!("held {} by {h} at tick {}: holder {at:?}, original {:?}", p.id, p.tick, (p.x, p.y, p.level));
                }
                continue;
            }
            let same = (o.x16, o.y16, o.level) == (p.x, p.y, p.level);
            notes.note(if same { "objects: an object's place at a tick's end natively the original's (not a person)" } else { "objects: an object's place at a tick's end NOT the original's (not a person)" });
            if !same && std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                println!("place of {} at tick {}: native {:?}, original {:?}", p.id, p.tick, (o.x16, o.y16, o.level), (p.x, p.y, p.level));
            }
        }
        // The disc's node (its operand in the file's order); SetToNext as the log's tree has it,
        // CreateObjectInstance by the disc's (its maker's tree has no record in the log).
        let disc_node = || step_object[i].and_then(|o| lib.tree(Some(o), s.tree)).and_then(|t| lib.tree_at(t).nodes.get(s.node as usize)).copied();
        let op = match raw_of(s) {
            Some(raw) if u16::from_be_bytes([raw[0], raw[1]]) & 0x7FFF == 31 && raw[2] != raw[3] => 31,
            _ if s.look == b'X' && disc_node().is_some_and(|n| matches!(n.opcode, 42 | 18)) => disc_node().unwrap().opcode,
            _ if disc_node().is_some_and(|n| matches!(n.opcode, 43 | 4 | 46)) => disc_node().unwrap().opcode,
            _ => continue,
        };
        let Some(node) = disc_node() else {
            continue;
        };
        if op == 31 && node.operand[4] & 0x80 != 0 && node.operand[5] != scope::STACK_OBJECT {
            continue;
        }
        // The objects made and removed, as the original's list shows them, replayed up to this
        // step's tick (each new id checked against the world's rule). An 'L' record is written
        // at the start of the next tick under the tick it ends, so a step sees the lists of the
        // ticks before its own.
        while li < log.lists.len() && log.lists[li].tick < s.tick {
            let l = &log.lists[li];
            li += 1;
            let now: BTreeSet<i16> = l.ids.iter().copied().collect();
            let gone: Vec<ObjectKey> = lw.order().iter().copied().filter(|&k| !now.contains(&lw.get(k).id())).collect();
            for k in gone {
                lw.remove(k);
                notes.note("objects: removed");
            }
            for &id in l.ids.iter().rev() {
                if lw.find(id).is_some() {
                    continue;
                }
                let want = lw.next_id();
                // its file: that of the first step after it appears that reads its own variables
                // (scopes 0, 3, 8 on its pointer, which the list has), and the GUID of the file's
                // definition
                let mut o = Object::new();
                let own = |s: &StepRec, ptr: u32| s.lookups.iter().any(|v| v.object == ptr && matches!(v.scope, 0 | 3 | 8));
                if let Some(ptr) = l.ids.iter().position(|&x| x == id).map(|p| l.objects[p]) {
                    made_ptr.insert(id, ptr);
                    o.file = log.steps.iter().enumerate().find(|(i, s)| s.tick >= l.tick && own(s, ptr) && step_object[*i].is_some()).and_then(|(i, _)| step_object[i]);
                    if let Some(d) = o.file.and_then(|f| lib.file(f).definitions.first()) {
                        o.guid = d.guid();
                        o.definition = lib.definition(o.guid);
                    }
                }
                if std::env::var_os("LOCKSTEP_SHOW_LISTS").is_some() {
                    println!("made {id} at tick {}: file {:?}, guid {:08x}", l.tick, o.file.map(|f| &lib.file(f).path), o.guid);
                }
                lw.make_object(o);
                notes.note(if want == id { "objects: made, the id the same" } else { "objects: made, the id DIFFERENT" });
                if want != id {
                    let k = lw.find(want).unwrap();
                    lw.get_mut(k).data[DATA_OBJECT_ID] = id;
                    lw.reindex(k);
                }
            }
            let order: Vec<i16> = lw.objects().map(Object::id).collect();
            notes.note(if order == l.ids { "objects: the list's order the same" } else { "objects: the list's order DIFFERENT" });
            if order != l.ids && std::env::var_os("LOCKSTEP_SHOW_LISTS").is_some() {
                let first = order.iter().zip(&l.ids).position(|(a, b)| a != b).unwrap_or(0);
                println!("list at tick {}: native {:?} log {:?} (from {first}, of {} and {})", l.tick, &order[first..(first + 6).min(order.len())], &l.ids[first..(first + 6).min(l.ids.len())], order.len(), l.ids.len());
            }
        }
        if op == 4 || op == 46 {
            // Grab and Snap natively on this world: the actor's place as the log has it last
            // before the step (persons' places are routing's, not this world's), then the moved
            // objects against the places the log has next
            set_made_fields(&mut lw, &made_ptr, i);
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            if let Some(p) = log.places.iter().rfind(|p| p.id == me && p.tick < s.tick)
                && let Some(k) = lw.find(me)
            {
                let o = lw.get_mut(k);
                (o.x16, o.y16, o.level) = (p.x, p.y, p.level);
            }
            let next = |id: i16| log.places.iter().find(|p| p.id == id && p.tick >= s.tick).map(|p| (p.x, p.y, p.level));
            let name = if op == 4 { "Grab" } else { "Snap" };
            if op == 46 && node.operand[0] != 0 {
                notes.note(&format!("objects: Snap kind {}: not modelled", node.operand[0]));
                continue;
            }
            if next(me).is_none() && op == 46 {
                notes.note("objects: Snap: no place logged after it");
                continue;
            }
            let same = if op == 4 {
                grab(&mut lw, me, s.stack_object).map(|(id, x, y, l)| next(id) == Some((x, y, l)))
            } else {
                let held: Vec<i16> = lw.objects().filter(|o| o.id() != me && o.data.get(DATA_CONTAINER) == Some(&me)).map(Object::id).collect();
                snap(&node, &mut lw, me, s.stack_object).map(|(x, y, l)| next(me) == Some((x, y, l)) && held.iter().all(|&h| next(h) == Some((x, y, l))))
            };
            if same != Some(true) && std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                let so = lw.find_object(s.stack_object).map(|o| (o.x16, o.y16, o.level, o.file.map(|f| lib.file(f).path.clone())));
                println!("{name} at tick {} by {me} operand {:02x?}: stack object {} {so:?}; me next {:?}; me now {:?}", s.tick, node.operand, s.stack_object, next(me), lw.find_object(me).map(|o| (o.x16, o.y16)));
            }
            notes.note(&match same {
                Some(true) => format!("objects: {name} natively, the moved objects' places the original's"),
                Some(false) => format!("objects: {name} natively, NOT the original's"),
                None => format!("objects: {name}: its objects not found natively"),
            });
            continue;
        }
        if op == 43 {
            // DropOnto natively on this world: the object the actor holds to the stack object's
            // tile centre, against the place the log has for it next
            set_made_fields(&mut lw, &made_ptr, i);
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            let Some((id, x, y, level)) = drop_onto(&mut lw, me, s.stack_object) else {
                notes.note("objects: DropOnto: nothing held or no stack object natively");
                continue;
            };
            let logged = log.places.iter().find(|p| p.id == id && p.tick >= s.tick).map(|p| (p.x, p.y, p.level));
            notes.note(if logged == Some((x, y, level)) { "objects: DropOnto natively, the held object and its place the original's" } else { "objects: DropOnto natively, NOT the original's" });
            continue;
        }
        if op == 18 {
            // RemoveObjectInstance natively on this world: operand byte 0 1 the stack object, 0
            // the step's own object (its sim's: the object whose own variables it reads most);
            // the next list must then lack it
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            let Some(id) = remove_object(&node, &mut lw, me, s.stack_object) else {
                notes.note("objects: RemoveObjectInstance: its object not found natively");
                continue;
            };
            let next = log.lists.iter().find(|l| l.tick >= s.tick);
            notes.note(if next.is_some_and(|l| !l.ids.contains(&id)) {
                "objects: RemoveObjectInstance natively, the object the original removed"
            } else {
                "objects: RemoveObjectInstance natively, NOT an object the original removed"
            });
            continue;
        }
        if op == 42 {
            // CreateObjectInstance natively on this world: the new object's id against the
            // maker's stack object after, its GUID against the file its own steps run (the next
            // list then finds it made)
            let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
            let Some(k) = create_object(&node, &mut lw, me, s.stack_object) else {
                notes.note("objects: CreateObjectInstance: no definition of the GUID");
                continue;
            };
            let (id, guid) = (lw.get(k).id(), lw.get(k).guid);
            if std::env::var_os("LOCKSTEP_SHOW_DROPS").is_some() {
                let me = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).copied().unwrap_or(-1);
                let at = |id: i16| lw.find_object(id).map(|o| (o.id(), o.x16, o.y16, o.level, o.data.get(1).copied()));
                let first = log.places.iter().find(|p| p.id == id && p.tick + 1 >= s.tick).map(|p| (p.tick, p.x, p.y, p.level));
                let slot0 = lw.find_object(s.stack_object).and_then(|o| {
                    let d = o.definition.map(|d| lib.def(d))?;
                    lib.file(o.file?).slots.get(&d.slot_table()).map(|t| t.iter().filter(|s| s.kind != 3).map(|s| (s.kind, s.x, s.y, s.z, s.facing)).take(2).collect::<Vec<_>>())
                });
                println!("made {id} at tick {} operand {:02x?}: maker {:?} (me {:?}), stack object before {:?} (first container slots {slot0:?}); its first place {first:?}", s.tick, node.operand, at(me), me, at(s.stack_object));
            }
            notes.note(if id == s.stack_object_after { "objects: CreateObjectInstance natively, the id the original's" } else { "objects: CreateObjectInstance natively, the id DIFFERENT" });
            // the object's pointer: the one the next list has at that id
            let ptr = log.lists.iter().find(|l| l.tick >= s.tick).and_then(|l| l.ids.iter().position(|&x| x == id).map(|p| l.objects[p]));
            let own = |t: &StepRec, ptr: u32| t.lookups.iter().any(|v| v.object == ptr && matches!(v.scope, 0 | 3 | 8));
            let file = ptr.and_then(|p| log.steps.iter().enumerate().skip(i.saturating_sub(400)).find(|(j, t)| own(t, p) && step_object[*j].is_some()).and_then(|(j, _)| step_object[j]));
            let file_guid = file.and_then(|f| lib.file(f).definitions.first()).map(|d| d.guid());
            notes.note(if file_guid == Some(guid) { "objects: CreateObjectInstance natively, the GUID its own steps' file's" } else { "objects: CreateObjectInstance natively, the GUID NOT its steps' file's" });
            if let Some(p) = ptr {
                made_ptr.insert(id, p);
            }
            continue;
        }
        let mut f = Frame { params: s.params.clone(), locals: s.locals.clone(), stack_object: s.stack_object, ..Default::default() };
        if node.operand[4] & 0x7F == 6 {
            set_made_fields(&mut lw, &made_ptr, i);
        }
        // me, the step's own object (kind 9 searches beside it)
        let me_key = sim_object.get(&s.sim).and_then(|p| ptr_id.get(p)).and_then(|&id| lw.find(id)).unwrap_or(nobody);
        let r = set_to_next(&node, &mut ScriptVariables::new(&mut lw, me_key, &mut f));
        let Some(raw) = raw_of(s) else { continue };
        let orig_true = s.after == raw[2] as u16;
        let kind = format!("SetToNext kind {}", node.operand[4] & 0x7F);
        if r == PrimResult::Stop {
            notes.note(&format!("{kind}: not modelled"));
            continue;
        }
        let same = (r == PrimResult::True) == orig_true && (!orig_true || f.stack_object == s.stack_object_after);
        let ex = buf(
            format!(
                "tick {} from {}: original {} {}, native {} {}",
                s.tick,
                s.stack_object,
                if orig_true { "true" } else { "false" },
                s.stack_object_after,
                if r == PrimResult::True { "true" } else { "false" },
                f.stack_object
            ),
            128,
        );
        // While the lot loads (its first ticks) the original's list is still being built, in
        // another order (a global init tree walks the objects of a GUID in ascending id order
        // then); the native world has the finished list: counted apart.
        let verdict = if same {
            ": same"
        } else if s.tick < 10 {
            ": different while the lot loads (ticks 0-9)"
        } else {
            ": DIFFERENT"
        };
        notes.note_ex(&format!("{kind}{verdict}"), &ex);
        if !same && s.tick >= 10 && shown < std::env::var("LOCKSTEP_SHOW_N").ok().and_then(|v| v.parse().ok()).unwrap_or(12) {
            shown += 1;
            let o = &node.operand;
            let guid = u32::from_le_bytes([o[0], o[1], o[2], o[3]]);
            let pos = |id: i16| lw.objects().position(|x| x.id() == id).map_or(-1, |p| p as i64);
            println!(
                "  {kind} tick {} tree {} node {} GUID {guid:08x}: from {} (at {}), original {} (at {}, GUID {:08x})",
                s.tick,
                alt_hex(s.tree as u64),
                s.node,
                s.stack_object,
                pos(s.stack_object),
                s.stack_object_after,
                pos(s.stack_object_after),
                lw.find_object(s.stack_object_after).map_or(0, |x| x.guid)
            );
        }
    }
}

/// The lights as the engine makes them with its own lamps (`lamps::on_lot`: places from the
/// objects' poses, no logged place): the lamps are light objects in the world list's order, each
/// in its tile's room, sorted with the windows (`sort_lights`); then, against the log's last
/// rebuild `b`, the own lights in order (place, reach, colour), the light grid built after the
/// rebuild before it (every cell room's four lights) and each window fill's nearest lamps and
/// mix. A rebuild or fill from before the lamps were placed (the original's lamp lights sit near
/// the origin then) is counted apart.
#[allow(clippy::too_many_arguments)]
fn lamps_from_the_engine(
    log: &housefire::tree_log::Log,
    b: &housefire::tree_log::LightRebuildRec,
    rooms: usize,
    objs: &[(housefire::light::LightKind, i32, i32)],
    info: &[((f32, f32), u32, i32)],
    other_order: &[usize],
    lamps: &[housefire::lamps::Lamp],
    lw: &World,
    walls: &housefire::routing::Walls,
) {
    use housefire::light::{self, LightKind};
    let by_leader: BTreeMap<i16, &housefire::lamps::Lamp> = lamps.iter().map(|l| (l.leader, l)).collect();
    let mut all = objs.to_vec();
    let mut lamp_lights = Vec::new();
    for o in lw.objects().filter(|o| o.on_lot()) {
        let Some(l) = by_leader.get(&o.id()) else { continue };
        let room = i32::from(walls.room(o.x16 >> 4, o.y16 >> 4).wrapping_sub(1));
        for light in &l.lights {
            all.push((LightKind::Lamp, room, room));
            lamp_lights.push(*light);
        }
    }
    let (ranges, other, own) = light::sort_lights(rooms, &all);
    debug_assert_eq!(other.len(), other_order.len());
    let mut lights = vec![([0.0f32; 3], 0.0f32); 512];
    for (k, &i) in other.iter().enumerate() {
        let ((x, y), kind, me) = info[i];
        if let Some((reach, _, Some(h))) = light::window_light_params(kind, me != 0) {
            lights[k] = ([x, y, h], reach);
        }
    }
    let own_lights: Vec<housefire::lamps::LampLight> = own.iter().map(|&i| lamp_lights[i - objs.len()]).collect();
    for (k, l) in own_lights.iter().enumerate() {
        lights[256 + k] = (l.at, l.reach);
    }
    // the own lights in order against the rebuild's
    let theirs = &b.lights[(b.other as usize).min(b.lights.len())..];
    let f = f32::from_bits;
    let (mut placed, mut coloured) = (0, 0);
    for (mine, t) in own_lights.iter().zip(theirs) {
        placed += usize::from([f(t[0]), f(t[1]), f(t[2]), f(t[3])].map(f32::to_bits) == [mine.at[0], mine.at[1], mine.at[2], mine.reach].map(f32::to_bits));
        coloured += usize::from([t[4], t[5], t[6]] == mine.colour.map(f32::to_bits));
    }
    if std::env::var_os("LOCKSTEP_LAMPS_SHOW").is_some() {
        for t in theirs {
            println!("  log own light obj {:08x}: ({}, {}, {}) reach {} colour ({}, {}, {})", t[7], f(t[0]), f(t[1]), f(t[2]), f(t[3]), f(t[4]), f(t[5]), f(t[6]));
        }
        for (k, m) in own_lights.iter().enumerate() {
            println!("  engine own light {k}: node {} {:?} reach {} colour {:?}", m.node, m.at, m.reach, m.colour);
        }
        for t in theirs {
            let at = [f(t[0]), f(t[1]), f(t[2])];
            if own_lights.iter().any(|m| m.at == at) {
                continue;
            }
            let rec = log.object_lights.iter().rfind(|o| o.lights.iter().any(|l| l.light == t[7]));
            println!("  unmatched log light {:08x} at {at:?}: record {:?}", t[7], rec.map(|o| (o.tick, o.object, o.lights.iter().map(|l| (l.kind, l.node, l.light == t[7])).collect::<Vec<_>>())));
            for o in lw.objects().filter(|o| o.on_lot() && (o.x16 as f32 / 16.0 - at[0]).abs() < 1.5 && (o.y16 as f32 / 16.0 - at[1]).abs() < 1.5) {
                println!("    near: object {} guid {:08x} leader {} at ({}, {}) file {:?}", o.id(), o.guid, o.leader, o.x16 as f32 / 16.0, o.y16 as f32 / 16.0, o.file.map(|f| lw.library().file(f).path.clone()));
            }
        }
    }
    println!("own lights from the engine's lamps: the engine's {}, the log's {}; in order the same place and reach {placed}, colour {coloured}", own_lights.len(), theirs.len());
    // the grid built after the rebuild
    if let Some(g) = b.builds_before.checked_sub(1).and_then(|i| log.grid_builds.get(i)) {
        let mut cells = light::CellRooms::default();
        for (x, y, r) in light::cell_room_adds((walls.size().0.min(39), walls.size().1.min(39)), |x, y| walls.room(x, y).wrapping_sub(1) as u8, |x, y| walls.diagonal_bits(x, y)) {
            cells.add(x, y, r);
        }
        // (a light on a stream, the laser show's, has its reach from the stream, not decoded: the
        // engine's is the stream's default; counted again with such lights as the build has them)
        if std::env::var_os("LOCKSTEP_LAMPS_SHOW").is_some() {
            for k in 0..own_lights.len() {
                println!("  build light {k}: {:?} engine {:?}", g.lights.get(256 + k), lights[256 + k]);
            }
            println!("  ranges engine {ranges:?}\n  ranges build  {:?}", &g.ranges[..rooms]);
        }
        let mut as_built = lights.clone();
        let mut moving = 0;
        for k in 0..own_lights.len() {
            if let Some(l) = g.lights.get(256 + k).filter(|_| own_lights[k].animated) {
                as_built[256 + k] = *l;
                moving += 1;
            }
        }
        for (name, set) in [("the engine's lamps and windows", &lights), ("the lights on streams as built", &as_built)] {
            let (mut cell_rooms, mut lit_same) = (0, 0);
            for (ci, cell) in g.cells.iter().enumerate() {
                let (x, y) = (2 + ci / 36, 2 + ci % 36);
                for (k, &id) in cells.at(x as i32, y as i32).unwrap_or([0xFF; 4]).iter().enumerate() {
                    if id == 0xFF {
                        break;
                    }
                    cell_rooms += 1;
                    let logged: [u16; 4] = {
                        let (a, c) = (cell[1 + 2 * k], cell[2 + 2 * k]);
                        [(a >> 16) as u16, a as u16, (c >> 16) as u16, c as u16]
                    };
                    let Some(r) = ranges.get(usize::from(id)) else { continue };
                    let point = [x as f32, y as f32, light::GRID_Z];
                    if light::cell_lights(point, (r[2], r[3]), (r[0], r[1]), set) == Some(logged) {
                        lit_same += 1;
                    }
                }
            }
            println!("grid build at tick {} from {name} ({moving} lights on streams): cell rooms {cell_rooms}, the four lights the same {lit_same}", g.tick);
        }
    }
    // each window fill: its nearest lamps (ranked over its other side's room's own range) and the
    // mix of them, from the engine's lamps, where the grid build before it had them placed
    let (mut fills, mut before, mut list_same, mut mix_same, mut colour_same) = (0, 0, 0, 0, 0);
    for w in &log.window_fills {
        let Some(g) = log.grid_builds.iter().rev().find(|g| g.tick < w.tick) else {
            before += 1;
            continue;
        };
        let Some(&i) = other.get(w.light as usize) else { continue };
        let here = (0..own_lights.len()).all(|k| g.lights.get(256 + k).is_some_and(|l| l.0.map(f32::to_bits) == lights[256 + k].0.map(f32::to_bits) && l.1.to_bits() == lights[256 + k].1.to_bits()));
        if !here {
            before += 1;
            continue;
        }
        let Some(r) = usize::try_from(all[i].2).ok().and_then(|room| ranges.get(room)) else { continue };
        let Some(near) = light::nearest_lights(w.at, (r[2], r[3]), &lights) else { continue };
        let ids: Vec<u32> = near.iter().map_while(|s| s.map(u32::from)).collect();
        fills += 1;
        list_same += usize::from(ids == w.near.iter().map(|x| x.0).collect::<Vec<_>>());
        let lamps_near: Vec<_> = ids.iter().map(|&id| (lights[id as usize].0, lights[id as usize].1, own_lights[id as usize - 256].colour)).collect();
        let mix = light::window_mix(w.room, w.sun, w.at, &lamps_near);
        let colour = mix.map(|v| v * w.base);
        mix_same += usize::from(mix.map(f32::to_bits) == w.mix.map(f32::to_bits));
        colour_same += usize::from(colour.map(f32::to_bits) == w.colour.map(f32::to_bits));
    }
    println!("window fills from the engine's lamps: {fills} after a grid build with the lamps placed ({before} before any build or with them unplaced): nearest list the same {list_same}, mix {mix_same}, colour {colour_same}");
    // each LightGrid_Sum point: its cell's lights as the log has them, the lamps (found by their
    // light objects, the rebuild's own lights in order) replaced by the engine's place, reach and
    // colour, summed (`point_light`) against the logged colour; a call whose lamps are not yet at
    // the engine's places or reach (unplaced, or the light on a stream) is counted apart
    let own_objects: Vec<u32> = theirs.iter().map(|t| t[7]).collect();
    let (mut points, mut sum_same, mut unplaced, mut state) = (0, 0, 0, 0);
    for g in &log.light_grids {
        for (x, y, words, out) in &g.points {
            let mut engine_colour = true;
            let mut at_engine = true;
            let cell: Vec<light::Light> = words
                .iter()
                .map(|w| {
                    let Some(k) = own_objects.iter().position(|&o| o == w[7]).filter(|&k| k < own_lights.len()) else { return light::Light::from_words(w) };
                    let (l, logged) = (&own_lights[k], light::Light::from_words(w));
                    at_engine &= [logged.x, logged.y, logged.height].map(f32::to_bits) == l.at.map(f32::to_bits) && logged.reach.to_bits() == l.reach.to_bits();
                    engine_colour &= logged.colour.map(f32::to_bits) == l.colour.map(f32::to_bits);
                    light::Light { x: l.at[0], y: l.at[1], height: l.at[2], reach: l.reach, colour: l.colour }
                })
                .collect();
            if !at_engine {
                unplaced += 1;
                continue;
            }
            if !engine_colour {
                state += 1;
                continue;
            }
            points += 1;
            let mine = light::point_light(*x as f32, *y as f32, &cell);
            sum_same += usize::from(mine.map(f32::to_bits) == out.map(f32::to_bits));
        }
    }
    println!("light grid sums from the engine's lamps: {points} points the lamps of which are at the engine's places and colours: the sum bit for bit {sum_same} (DIFFERENT -> {}); {unplaced} points with a lamp not at the engine's place or reach (before the lamps were placed, or on a stream), {state} with a lamp of another animation state", points - sum_same);
}
