//! The variables check: the native object model against the original's lookups.

use crate::*;

/// The variables: the native object model against the original's lookups.
///
/// A native object for each object the original asks for or names as a stack object. Each
/// variable cell takes its value from the log the first time it is seen; after that the native
/// value must be what the log reads, unless something other than a script's expression wrote it
/// (the engine, or a primitive not modelled yet: counted apart). And each cell must always be
/// the same address in the original, and each address the same cell: that checks what the
/// scopes name.
pub fn variables(lib: &Library, log: &housefire::tree_log::Log, step_object: &[Option<FileId>], lot: &World, notes: &mut Notes) {
    let mut world = World::new(lib);
    // the neighbourhood's neighbours (scopes 24 and 32 look them up)
    world.neighbours = lot.neighbours.clone();
    let mut native: BTreeMap<u32, ObjectKey> = BTreeMap::new(); // the original's object pointer -> native object
    let mut cell_addr: HashMap<CellKey, u32> = HashMap::new();
    let mut addr_cell: BTreeMap<u32, CellKey> = BTreeMap::new();
    let mut seen: HashMap<CellKey, bool> = HashMap::new();
    let mut last_writer: BTreeMap<u32, u32> = BTreeMap::new(); // address -> the caller that last had its pointer
    let mut addr_used: BTreeMap<u32, usize> = BTreeMap::new(); // address -> the last step that reached it
    let mut bound_at: HashMap<CellKey, u32> = HashMap::new(); // a cell's binding -> the tick it was made
    // the ticks each id was made again (it appears in the original's list after being absent):
    // a cell of that id bound before is another object's memory now
    let mut remade: BTreeMap<i16, Vec<u32>> = BTreeMap::new();
    for w in log.lists.windows(2) {
        for id in w[1].ids.iter().filter(|id| !w[0].ids.contains(id)) {
            remade.entry(*id).or_default().push(w[1].tick);
        }
    }
    let mut why: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new(); // a difference -> (tick band, caller) counts
    let mut loaded_at = u32::MAX; // the tick the current lot started loading
    let mut shown = 0;
    let differ = |notes: &mut Notes, why: &mut BTreeMap<String, BTreeMap<String, i64>>, loaded_at: u32, k0: &str, s: &StepRec, v: &LookupRec| {
        // While a lot loads the engine sets its objects up itself (their ids among them):
        // counted apart.
        let loading = loaded_at != u32::MAX && s.tick >= loaded_at && s.tick < loaded_at.wrapping_add(25);
        let k = if loading { format!("{} (while a lot loads)", &k0[..k0.find(" DIFFERENT").unwrap_or(k0.len())]) } else { k0.to_string() };
        notes.note(&k);
        let b = buf(format!("tick {}-{} caller {:x}", s.tick / 20 * 20, s.tick / 20 * 20 + 19, v.caller), 64);
        *why.entry(k).or_default().entry(b).or_default() += 1;
    };
    let object_at = |world: &mut World, native: &mut BTreeMap<u32, ObjectKey>, ptr: u32, file: Option<FileId>| -> ObjectKey {
        if let Some(&k) = native.get(&ptr) {
            return k;
        }
        let mut o = Object::new();
        o.file = file;
        let k = world.add(o, false);
        native.insert(ptr, k);
        k
    };
    // Where a stack-object scope's variable is in the original: back from its address to the
    // object's (the layouts in docs/objects.md).
    let owner_of = |v: &LookupRec| -> Option<u32> {
        let a = if v.ptr != 0 { v.ptr } else { v.fptr };
        if a == 0 {
            return None; // the value alone was asked for
        }
        let d = v.data as i32 as u32;
        match v.scope {
            4 => Some(a.wrapping_sub(0x26).wrapping_sub(d.wrapping_mul(2))),
            15 => Some(a.wrapping_add(0x598).wrapping_sub(d.wrapping_mul(4))),
            19 => Some(a.wrapping_add(0x638).wrapping_sub(d.wrapping_mul(2))),
            _ => None,
        }
    };
    // A lot loaded: the ticks at which 20 or more objects never seen before ask for variables
    // (the dream at 0, the club at 2799 in the new-game route).
    let mut lot_ticks: BTreeSet<u32> = BTreeSet::new();
    {
        let mut seen_objects: BTreeSet<u32> = BTreeSet::new();
        let mut fresh: BTreeMap<u32, i32> = BTreeMap::new();
        for s in &log.steps {
            for v in &s.lookups {
                if seen_objects.insert(v.object) {
                    *fresh.entry(s.tick).or_default() += 1;
                }
            }
        }
        lot_ticks.extend(fresh.iter().filter(|&(_, &n)| n >= 20).map(|(&t, _)| t));
    }
    let mut lot_tick = u32::MAX;
    for (i, s) in log.steps.iter().enumerate() {
        if s.lookups.is_empty() {
            continue;
        }
        // (a load's objects may come over two ticks: one lot)
        if lot_ticks.contains(&s.tick) && (lot_tick == u32::MAX || s.tick > lot_tick.wrapping_add(1)) {
            if lot_tick != u32::MAX {
                notes.note("variables: a lot loaded (the native world cleared)");
            }
            world.clear();
            native.clear();
            cell_addr.clear();
            addr_cell.clear();
            seen.clear();
            last_writer.clear();
            addr_used.clear();
        }
        if lot_ticks.contains(&s.tick) {
            if lot_tick == u32::MAX || s.tick > lot_tick.wrapping_add(1) {
                loaded_at = s.tick;
            }
            lot_tick = s.tick;
        }
        let mut frame = Frame { params: s.params.clone(), locals: s.locals.clone(), stack_object: s.stack_object, file: step_object[i], ..Default::default() };
        for v in &s.lookups {
            let me = object_at(&mut world, &mut native, v.object, step_object[i]);
            world.get_mut(me).person |= v.scope == 14 || v.scope == 18;
            let sc = format!("scope {}", v.scope);
            if v.flags & 1 == 0 {
                notes.note(&format!("variables: {sc}: the original's lookup failed"));
                continue;
            }
            if v.scope == 7 || v.scope == 26 {
                if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                    continue;
                }
                let g = ScriptVariables::new(&mut world, me, &mut frame).get(v.scope as u8, v.data);
                if g.is_some_and(|g| g.value == v.value) {
                    notes.note(&format!("variables: {sc}: same"));
                } else {
                    differ(notes, &mut why, loaded_at, &format!("variables: {sc}: DIFFERENT"), s, v);
                }
                continue;
            }
            // A stack object the native world does not have yet: made from the original's.
            if let Some(q) = owner_of(v) {
                // the object at this address, met before without its id (made from another
                // lookup): it is this id's now (an id reused: the marker 197 after the chips, at
                // another address)
                if let Some(&b) = native.get(&q)
                    && world.get(b).id() != s.stack_object
                    && world.get(b).id() == 0
                {
                    let o = world.get_mut(b);
                    if o.data.len() <= DATA_OBJECT_ID {
                        o.data.resize(DATA_OBJECT_ID + 1, 0);
                    }
                    o.data[DATA_OBJECT_ID] = s.stack_object;
                    world.reindex(b);
                }
                let so = match native.get(&q).copied().filter(|&b| world.get(b).id() == s.stack_object).or(world.find(s.stack_object)) {
                    Some(so) => so,
                    None => {
                        let n = object_at(&mut world, &mut native, q, None);
                        let o = world.get_mut(n);
                        if o.data.len() <= DATA_OBJECT_ID {
                            o.data.resize(DATA_OBJECT_ID + 1, 0);
                        }
                        o.data[DATA_OBJECT_ID] = s.stack_object;
                        world.reindex(n);
                        n
                    }
                };
                world.get_mut(so).person |= v.scope == 15 || v.scope == 19;
                let bound = native.get(&q).copied();
                if bound == Some(so) {
                    notes.note("variables: stack object the same");
                } else {
                    differ(notes, &mut why, loaded_at, "variables: stack object DIFFERENT", s, v);
                    if shown < 6 {
                        shown += 1;
                        let b = bound.map_or_else(|| "(nil)".to_string(), |b| format!("#{}", b.0));
                        println!(
                            "  stack object {}: by id native #{} (id {}), at the original's address {:x} native {} (id {}); scope {} caller {:x}",
                            s.stack_object,
                            so.0,
                            world.get(so).id(),
                            q,
                            b,
                            bound.map_or(-1, |b| world.get(b).id() as i32),
                            v.scope,
                            v.caller
                        );
                    }
                }
                if bound.is_none() {
                    native.insert(q, so);
                }
            }
            let Some(cell) = ScriptVariables::new(&mut world, me, &mut frame).cell(v.scope as u8, v.data) else {
                // a scope with no cell of its own (a value the engine works out): its value
                // natively, where the native variables give one (study first: counted apart)
                // an object whose steps name no file (a multi-tile part's): its id's file and
                // definition in the lot (a definition's fields, scope 21)
                for k in [Some(me), world.find(frame.stack_object)].into_iter().flatten() {
                    if let Some(o) = world.get_live(k)
                        && o.file.is_none()
                        && let Some(l) = lot.find_object(o.id())
                    {
                        let (file, def) = (l.file, l.definition);
                        let o = world.get_mut(k);
                        o.file = file;
                        o.definition = def;
                        notes.note("variables: an object's file and definition from the lot (its steps name none)");
                    }
                }
                let got = ScriptVariables::new(&mut world, me, &mut frame).get(v.scope as u8, v.data);
                notes.note(&match got {
                    Some(g) if v.flags & 1 != 0 && g.value == v.value => format!("variables: {sc}: no cell, the value the same natively"),
                    Some(g) if v.flags & 1 != 0 => {
                        if std::env::var("LOCKSTEP_SHOW_UNMODELLED").ok().and_then(|x| x.parse::<i16>().ok()) == Some(v.scope) {
                            let at = |k: Option<ObjectKey>| k.and_then(|k| world.get_live(k)).map(|o| (o.id(), o.x16, o.y16, o.level, o.person));
                            eprintln!("other scope {} datum {}: native {}, log {} at tick {}: me {:?}, stack object {} {:?}", v.scope, v.data, g.value, v.value, s.tick, at(Some(me)), frame.stack_object, at(world.find(frame.stack_object)));
                        }
                        format!("variables: {sc}: no cell, the value other natively (study)")
                    }
                    _ => {
                        if std::env::var("LOCKSTEP_SHOW_UNMODELLED").ok().and_then(|v| v.parse::<i16>().ok()) == Some(v.scope)
                            && let Some(o) = world.get_live(me)
                        {
                            let so = world.find_object(frame.stack_object);
                            eprintln!(
                                "unmodelled scope {} datum {} = {} at tick {}: me {} (person {}, def {:?}, file {:?}), stack object {} (def {:?})",
                                v.scope, v.data, v.value, s.tick, o.id(), o.person, o.definition, o.file, frame.stack_object, so.map(|o| o.definition)
                            );
                        }
                        format!("variables: {sc}: not modelled yet")
                    }
                });
                continue;
            };
            // The cell as the C++ tool keyed it: its table and index, every table of an object
            // the world does not have one (its null pointer).
            let key = CellKey(if cell.table.owner() == Some(None) { None } else { Some(cell.table) }, cell.index);
            let addr = if v.ptr != 0 { v.ptr } else { v.fptr };
            // Frame variables are the step's own: rebuilt from the log each step.
            let in_frame = v.scope == 9 || v.scope == 25 || v.scope == 10;
            if addr != 0 && !in_frame {
                // A binding unused for a long time is stale: the memory was reused, or the object
                // at a pointer replaced (another lot loaded). Forget it.
                let stale = |addr_used: &BTreeMap<u32, usize>, a: u32| addr_used.get(&a).is_none_or(|&u| i - u > 20000);
                if let Some(&c) = addr_cell.get(&addr)
                    && c != key
                    && stale(&addr_used, addr)
                {
                    cell_addr.remove(&c);
                    seen.remove(&c);
                    addr_cell.remove(&addr);
                    notes.note("variables: memory reused");
                }
                let owner_remade = cell.table.owner().flatten().and_then(|k| world.get_live(k)).is_some_and(|o| {
                    let t0 = bound_at.get(&key).copied().unwrap_or(0);
                    remade.get(&o.id()).is_some_and(|ts| ts.iter().any(|&t| t > t0 && t <= s.tick + 1))
                });
                if let Some(&a) = cell_addr.get(&key)
                    && a != addr
                    && (stale(&addr_used, a) || owner_remade)
                {
                    addr_cell.remove(&a);
                    seen.remove(&key);
                    cell_addr.remove(&key);
                    notes.note("variables: object replaced");
                }
                // An address first read while its object was not in the world yet (bound to no
                // owner: an engine-made object listed from the next tick, the club's 181 at tick
                // 6513) has no identity: once it is an object's cell, it is bound anew.
                if let Some(&c) = addr_cell.get(&addr)
                    && c.0.is_none()
                    && key.0.is_some()
                {
                    cell_addr.remove(&c);
                    seen.remove(&c);
                    addr_cell.remove(&addr);
                    notes.note("variables: an address read before its object was in the world, bound to it now");
                }
                let ca = cell_addr.get(&key).copied();
                let ac = addr_cell.get(&addr).copied();
                addr_used.insert(addr, i);
                if ca.is_none() && ac.is_none() {
                    cell_addr.insert(key, addr);
                    addr_cell.insert(addr, key);
                    bound_at.insert(key, s.tick);
                } else if ca.is_some_and(|a| a != addr) || ac.is_some_and(|c| c != key) {
                    if std::env::var_os("LOCKSTEP_SHOW_WHERE").is_some() {
                        let other = ac.map(|c| (c.0.and_then(|t| t.owner().flatten()).and_then(|k| world.get_live(k)).map(|o| o.id()), c.1, bound_at.get(&c).copied()));
                        eprintln!(
                            "where tick {} scope {} data {} me {} stack {} addr {addr:08x}: the cell (owner {:?}, index {}) bound to {ca:08x?} since {:?}; the address bound to {other:?}",
                            s.tick, v.scope, v.data, world.get(me).id(), frame.stack_object,
                            cell.table.owner().flatten().and_then(|k| world.get_live(k)).map(|o| o.id()), cell.index, bound_at.get(&key)
                        );
                    }
                    differ(notes, &mut why, loaded_at, &format!("variables: {sc}: where DIFFERENT"), s, v);
                } else {
                    notes.note(&format!("variables: {sc}: where the same"));
                }
            }
            if in_frame {
                if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                    continue; // the address only
                }
                let g = ScriptVariables::new(&mut world, me, &mut frame).get(v.scope as u8, v.data);
                if g.is_some_and(|g| g.value == v.value) {
                    notes.note(&format!("variables: {sc}: value the same"));
                } else {
                    differ(notes, &mut why, loaded_at, &format!("variables: {sc}: value DIFFERENT"), s, v);
                }
                continue;
            }
            // The value: seeded once, then the native one must hold. Only the expression asks for
            // the value; other primitives take the address alone (the log's value is then not
            // one).
            if v.caller != EXPR_LHS && v.caller != EXPR_RHS {
                if addr != 0 && v.flags & 6 != 0 {
                    last_writer.insert(addr, v.caller);
                }
                seen.insert(key, false); // it may write there: take the next value read as the new one
                continue;
            }
            let is_float = v.flags & 4 != 0 && v.flags & 2 == 0;
            let mut vars = ScriptVariables::new(&mut world, me, &mut frame);
            let got = vars.get(v.scope as u8, v.data);
            let set = |vars: &mut ScriptVariables| {
                if is_float {
                    vars.set_float(v.scope as u8, v.data, f32::from_bits(v.fvalue));
                } else {
                    vars.set(v.scope as u8, v.data, v.value);
                }
            };
            if !*seen.entry(key).or_insert(false) {
                set(&mut vars);
                seen.insert(key, true);
                notes.note(&format!("variables: {sc}: first seen"));
            } else {
                let same = if is_float {
                    got.and_then(|g| g.f).unwrap_or(0.0).to_bits() == v.fvalue
                } else {
                    got.is_some_and(|g| g.value == v.value)
                };
                if same {
                    notes.note(&format!("variables: {sc}: value the same"));
                } else {
                    let other = last_writer.get(&addr).is_some_and(|&w| w != EXPR_LHS && w != EXPR_RHS);
                    notes.note(&format!(
                        "variables: {sc}{}",
                        if other { ": changed by a primitive not modelled yet" } else { ": changed by the engine" }
                    ));
                    set(&mut vars);
                }
            }
            if addr != 0 && v.flags & 6 != 0 {
                last_writer.insert(addr, v.caller);
            }
        }
        // The expression's write: the native cell takes the value after (the expression itself is
        // checked above).
        if let (Some(e), Some(v)) = (&s.expr, s.lookups.last())
            && v.caller == EXPR_LHS
        {
            let me = object_at(&mut world, &mut native, v.object, step_object[i]);
            let mut vars = ScriptVariables::new(&mut world, me, &mut frame);
            if e.flags & 1 != 0 {
                vars.set(v.scope as u8, v.data, e.lhs_after);
            } else if e.flags & 2 != 0 {
                vars.set_float(v.scope as u8, v.data, f32::from_bits(e.lhs_float_after));
            }
        }
    }
    for (k, m) in &why {
        println!("{k}:");
        let mut top: Vec<(i64, &String)> = m.iter().map(|(w, &n)| (n, w)).collect();
        top.sort();
        top.reverse();
        for (n, w) in top.iter().take(4) {
            println!("    {n:>6}  {w}");
        }
    }
}
