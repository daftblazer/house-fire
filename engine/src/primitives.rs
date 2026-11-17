//! Primitives other than the expression (`expression`), each on the world (`world`).

use crate::bhav::{Node, TreeKind, tree_kind};
use crate::expression::Variables;
use crate::iff::Order;
use crate::interp::{Frame, Interpreter, PrimResult, Sleep};
use crate::save::{MotiveDelta, SavedQueueEntry};
use crate::simulator::SimHost;
use crate::world::{Object, ObjectKey, PresentationEvent, ScriptVariables, World, scope};

/// A 16-bit field of a node's operand at byte `i`, in the file's byte order.
fn field(n: &Node, i: usize) -> i16 {
    let b = [n.operand[i], n.operand[i + 1]];
    match n.order {
        Order::Little => i16::from_le_bytes(b),
        Order::Big => i16::from_be_bytes(b),
    }
}

/// A GUID in a node's operand, in the file's byte order (Bustin' Out's files are little-endian).
fn operand_guid(n: &Node) -> u32 {
    let b = [n.operand[0], n.operand[1], n.operand[2], n.operand[3]];
    match n.order {
        Order::Little => u32::from_le_bytes(b),
        Order::Big => u32::from_be_bytes(b),
    }
}

/// Random (opcode 8): a number from 0 up to the range (operand bytes 4-5 its datum, byte 6 its
/// scope) into the destination (bytes 0-1 its datum, byte 2 its scope); true. The numbers are
/// the engine's own (`World::rng`). Measured: every logged result within its range
/// (`docs/primitives.md`, "Random").
pub fn random(n: &Node, vars: &mut ScriptVariables) -> PrimResult {
    let Some(range) = vars.get(n.operand[6], field(n, 4)) else { return PrimResult::Stop };
    let v = if range.value > 0 { vars.world.rng.below(range.value as u32) as i16 } else { 0 };
    if vars.set(n.operand[2], field(n, 0), v) { PrimResult::True } else { PrimResult::Stop }
}

/// CompleteGoal (opcode 56), its old form (operand byte 6 bit 0, every use in the logs): whether
/// the player has met a goal of a house. The operand: bytes 0-1 the house's datum and byte 4 its
/// scope, bytes 2-3 the goal's datum and byte 5 its scope. True if the goal's mark is met
/// (`goals::Goals`), false if not; no such house or goal is the original's script error (70, 71:
/// `Stop`). Read from the original's 80211338. Not modelled (`None`): the new form, which asks
/// the goal manager and can wait for the player; and the original's game modes 1 and 2 (its
/// +0x468 of 8037BFA8, 0 in every session logged), in which every goal answers true.
pub fn complete_goal(n: &Node, vars: &mut ScriptVariables) -> Option<PrimResult> {
    if n.operand[6] & 1 == 0 {
        return None;
    }
    let house = vars.get(n.operand[4], field(n, 0));
    let goal = vars.get(n.operand[5], field(n, 2));
    let (Some(house), Some(goal)) = (house, goal) else { return Some(PrimResult::Stop) };
    let goals = &vars.world.goals;
    Some(match goals.mark(house.value, goal.value) {
        Some(m) if goals.is_met(m) => PrimResult::True,
        Some(_) => PrimResult::False,
        None => PrimResult::Stop,
    })
}

/// Dialog (opcode 36): opens a dialog for the player (a presentation event, the world's
/// dialog_object) and waits for the answer (`World::dialog_answer`): each step until then yields
/// and asks the pass to stop (`World::stop_pass`); the answer given, the node goes to its true
/// target. Measured (replay): all 2,471 waits and 4 answers of the session as the log's, every
/// answer true (the dialogs' one button); `docs/simulation.md`, "Dialogs".
pub fn dialog(n: &Node, world: &mut World, me: i16) -> PrimResult {
    if world.dialog_object != me {
        // opening it
        world.dialog_object = me;
        world.dialog_answer = None;
        world.presentation.push(PresentationEvent { opcode: n.opcode, object: me, operand: n.operand });
    }
    if world.dialog_answer.is_none() {
        world.stop_pass = true;
        return PrimResult::Yield;
    }
    world.dialog_object = 0;
    world.dialog_answer = None;
    PrimResult::True
}

/// A person's running motive changes, as one of its turns begins: each moves its motive by its
/// change, not past its stop value.
pub fn apply_motive_deltas(p: &mut Object) {
    for d in &p.motive_deltas {
        let Some(m) = usize::try_from(d.motive).ok().and_then(|i| p.motives.get_mut(i)) else { continue };
        if d.rate > 0.0 && *m < d.target {
            *m = (*m + d.rate).min(d.target);
        } else if d.rate < 0.0 && *m > d.target {
            *m = (*m + d.rate).max(d.target);
        }
    }
}

/// PlaySound (23), Update (7) and KillSounds (48): requests to the presentation, queued on the
/// world for the front ends (`World::presentation`); the step is true. In the logs they are true
/// every time (100, 168 and 2 uses) and never change their frame.
pub fn present(n: &Node, world: &mut World, me: i16) -> PrimResult {
    world.presentation.push(PresentationEvent { opcode: n.opcode, object: me, operand: n.operand });
    PrimResult::True
}

/// TestObjectType (opcode 32): whether the object whose id a variable holds (operand bytes 4-5
/// the datum, byte 6 the scope) is of the GUID in bytes 0-3. An id no object has: false.
/// Measured on the recompiled game (`docs/primitives.md`, "TestObjectType").
pub fn test_object_type(n: &Node, vars: &mut ScriptVariables) -> PrimResult {
    let Some(id) = vars.get(n.operand[6], field(n, 4)) else { return PrimResult::Stop };
    let same = vars.world.find_object(id.value).is_some_and(|x| x.guid == operand_guid(n));
    if same { PrimResult::True } else { PrimResult::False }
}

/// SetToNext (opcode 31): the next object after the target's in the world's list order (or the
/// first, if the target is 0) of a kind: 0 any object, 1 a Sim, 2 not a Sim, 3 the next part of
/// the target's multi-tile object, 4 an object of the GUID in operand bytes 0-3, 5 the next
/// neighbour, 6 an object whose data field 59 is the frame's first parameter. The target is the
/// stack object, or with flag 0x80 in byte 4 the variable of scope byte 5, datum byte 7. True
/// and the target set if there is one; false if not. Other kinds are not modelled yet: Stop.
/// Measured on the recompiled game (`docs/objects.md`, "SetToNext").
/// CreateObjectInstance (opcode 42), the object itself: one of the GUID in operand bytes 0-3
/// (little-endian), with its definition and file, 72 data fields, as many attributes as its
/// definition's field 56, taking the lowest id free and going in front of the list
/// (`World::make_object`). None if the GUID has no definition. Where it goes (byte 4), its init
/// tree (run inside the maker's step) and the maker's stack object are the caller's.
/// Where it goes, operand byte 4 (The Sims' numbering): 2 in the maker's hand (its place, held:
/// container field the maker), 4 the stack object's slot 0 (the centre of its tile), 6 out of
/// the world ((-16, -16), as an object off the lot); the others are not modelled (where the
/// object starts). Measured: the home log's three (`docs/primitives.md`, "CreateObjectInstance").
pub fn create_object(n: &Node, w: &mut World, me: i16, stack_object: i16) -> Option<ObjectKey> {
    let guid = u32::from_le_bytes([n.operand[0], n.operand[1], n.operand[2], n.operand[3]]);
    let k = make_of_guid(w, guid)?;
    place_made(n, w, k, me, stack_object);
    Some(k)
}

/// A new object of the GUID's definition (the first half of CreateObjectInstance): its file, 72
/// data fields, its definition's field-56 attributes, the lowest id free, in front of the list.
pub fn make_of_guid(w: &mut World, guid: u32) -> Option<ObjectKey> {
    let lib = w.library();
    let d = lib.definition(guid)?;
    let mut o = Object::new();
    o.guid = guid;
    o.definition = Some(d);
    o.file = Some(d.file);
    o.data = vec![0; 72];
    o.attributes = vec![0; lib.def(d).field(56) as usize];
    Some(w.make_object(o))
}

/// Where CreateObjectInstance puts the object it made, by operand byte 4 (the second half).
pub fn place_made(n: &Node, w: &mut World, k: ObjectKey, me: i16, stack_object: i16) {
    let at = match n.operand[4] {
        2 => w.find_object(me).map(|m| (m.x16, m.y16, m.level, Some(me))),
        4 => w.find_object(stack_object).map(|t| (((t.x16 >> 4) << 4) + 8, ((t.y16 >> 4) << 4) + 8, t.level, None)),
        6 => Some((-16, -16, w.get(k).level, None)),
        _ => None,
    };
    if let Some((x, y, level, held)) = at {
        let o = w.get_mut(k);
        (o.x16, o.y16, o.level) = (x, y, level);
        if let Some(h) = held {
            o.data[crate::world::DATA_CONTAINER] = h;
        }
    }
}

/// DropOnto (opcode 43, operand byte 4 a flag): the object the actor holds (the first whose
/// container field, data 2, is the actor) goes to the centre of the stack object's tile, on its
/// level, into its slot 0 (container the stack object, slot 0). Returns the dropped object's id
/// and new place.
/// Measured: the home log's four (the meal onto the counter and the stove, 202 onto the dining
/// table), each the place the log has next.
pub fn drop_onto(w: &mut World, me: i16, stack_object: i16) -> Option<(i16, i32, i32, i32)> {
    let to = w.find_object(stack_object)?;
    let (x, y, level) = (((to.x16 >> 4) << 4) + 8, ((to.y16 >> 4) << 4) + 8, to.level);
    let k = w.order().iter().copied().find(|&k| {
        let o = w.get(k);
        o.id() != me && o.data.get(crate::world::DATA_CONTAINER) == Some(&me)
    })?;
    let o = w.get_mut(k);
    o.x16 = x;
    o.y16 = y;
    o.level = level;
    // now in the stack object's slot 0 (scope 16 reads it there)
    o.data[crate::world::DATA_CONTAINER] = stack_object;
    if o.data.len() > crate::world::DATA_SLOT {
        o.data[crate::world::DATA_SLOT] = 0;
    }
    Some((o.id(), x, y, level))
}

/// Grab (opcode 4): the stack object goes to the actor's place and is held by it (its
/// container field, data 2, the actor). Returns the object's id and place. Measured: the home
/// log's two (the chips at 823, the meal at 1330), each the place the log has next.
pub fn grab(w: &mut World, me: i16, stack_object: i16) -> Option<(i16, i32, i32, i32)> {
    let at = w.find_object(me)?;
    let (x, y, level) = (at.x16, at.y16, at.level);
    let k = w.find(stack_object).filter(|_| stack_object != me)?;
    let o = w.get_mut(k);
    o.x16 = x;
    o.y16 = y;
    o.level = level;
    if o.data.len() <= crate::world::DATA_CONTAINER {
        o.data.resize(crate::world::DATA_CONTAINER + 1, 0);
    }
    o.data[crate::world::DATA_CONTAINER] = me;
    Some((stack_object, x, y, level))
}

/// Snap (opcode 46), operand byte 0 kind 0: the actor goes to the centre of the stack object's
/// tile, on its level, and what it holds (container field the actor) with it. Returns the
/// actor's place; None for the other kinds (5, 6 on the home log: not modelled) or no objects.
/// Measured: the home log's one of kind 0 (Sim 12 onto the dining chair at 4996, the meal it
/// holds with it).
pub fn snap(n: &Node, w: &mut World, me: i16, stack_object: i16) -> Option<(i32, i32, i32)> {
    if n.operand[0] != 0 {
        return None;
    }
    let to = w.find_object(stack_object)?;
    let (x, y, level) = (((to.x16 >> 4) << 4) + 8, ((to.y16 >> 4) << 4) + 8, to.level);
    let moved: Vec<ObjectKey> = w
        .order()
        .iter()
        .copied()
        .filter(|&k| {
            let o = w.get(k);
            o.id() == me || o.data.get(crate::world::DATA_CONTAINER) == Some(&me)
        })
        .collect();
    if moved.is_empty() {
        return None;
    }
    for k in moved {
        let o = w.get_mut(k);
        o.x16 = x;
        o.y16 = y;
        o.level = level;
        // the actor is now in the stack object's slot 0 (its slot-0 reads, scope 20, find it)
        if o.id() == me && o.data.len() > crate::world::DATA_SLOT {
            o.data[crate::world::DATA_CONTAINER] = stack_object;
            o.data[crate::world::DATA_SLOT] = 0;
        }
    }
    Some((x, y, level))
}

/// RemoveObjectInstance (opcode 18): the stack object (operand byte 0 1) or the object itself
/// (0) leaves the world; its id is free again. None if there is no such object. Measured: the
/// home log's three removals (the paper girl herself at 466, the chips by their own tree at
/// 1382, the marker at 1767), each the object the original's next list lacks.
pub fn remove_object(n: &Node, w: &mut World, me: i16, stack_object: i16) -> Option<i16> {
    let id = if n.operand[0] == 1 { stack_object } else { me };
    let k = w.find(id)?;
    w.remove(k);
    Some(id)
}

/// SetToNext by tile (kinds 8 and 9): false with the target kept when there is none.
/// Kind 8, the next object on the target's own tile, the target among them, in the list's order
/// from its back (the oldest first), round from the last (so one alone gives itself, true): 23
/// of 23 in the home log's lockstep (a tile of 49, 104, 12 in a chair, and 202 in 12's hand:
/// 104 → 12 → 49, what a Sim holds left out); false only when the target is no object. Kind 9, the next object beside me: the
/// four tiles round mine in the order +x, +y, -y, -x, after the one the target is on, each tile's
/// lowest id among the objects (not Sims) whose flags (data 8) lack 4, none a Sim holds (the home log's
/// counters' walks: 42 of 42 in its lockstep).
fn set_to_next_by_tile(kind: u8, sc: u8, data: i16, vars: &mut ScriptVariables) -> PrimResult {
    let Some(cur) = vars.get(sc, data) else { return PrimResult::Stop };
    let cur = cur.value;
    let w = &*vars.world;
    let tile = |o: &Object| (o.x16 >> 4, o.y16 >> 4, o.level);
    // what a Sim holds is not on its tile (a Sim in a chair is)
    let held = |x: &Object| x.data.get(crate::world::DATA_CONTAINER).is_some_and(|&c| c != 0 && w.find_object(c).is_some_and(|c| c.person));
    let next = if kind == 8 {
        let Some(t) = w.find_object(cur).map(tile) else { return PrimResult::False };
        let on: Vec<i16> = w.order().iter().rev().map(|&k| w.get(k)).filter(|x| tile(x) == t && (x.id() == cur || !held(x))).map(Object::id).collect();
        let at = on.iter().position(|&id| id == cur).unwrap_or(0);
        on.get((at + 1) % on.len().max(1)).copied()
    } else {
        let Some(me) = w.get_live(vars.me) else { return PrimResult::False };
        let (mx, my, ml) = tile(me);
        const SIDES: [(i32, i32); 4] = [(1, 0), (0, 1), (0, -1), (-1, 0)];
        let from = w.find_object(cur).and_then(|c| {
            let (cx, cy, _) = tile(c);
            SIDES.iter().position(|&(dx, dy)| (mx + dx, my + dy) == (cx, cy))
        });
        let first = from.map_or(0, |k| k + 1);
        SIDES[first.min(4)..].iter().find_map(|&(dx, dy)| {
            w.objects()
                .filter(|x| tile(x) == (mx + dx, my + dy, ml) && !x.person && x.data.get(8).is_some_and(|f| f & 4 == 0) && !held(x))
                .map(Object::id)
                .min()
        })
    };
    match next {
        Some(id) => {
            if vars.set(sc, data, id) { PrimResult::True } else { PrimResult::Stop }
        }
        None => PrimResult::False,
    }
}

pub fn set_to_next(n: &Node, vars: &mut ScriptVariables) -> PrimResult {
    let o = &n.operand;
    let kind = o[4] & 0x7F;
    let (sc, data) = if o[4] & 0x80 != 0 { (o[5], o[7] as i16) } else { (scope::STACK_OBJECT, 0) };
    let guid = operand_guid(n);
    if kind == 8 || kind == 9 {
        return set_to_next_by_tile(kind, sc, data, vars);
    }
    if kind > 6 {
        return PrimResult::Stop;
    }
    // Kind 6: an object whose data field 59 is the frame's first parameter (Castaway's
    // TrySetToNext, the same in Bustin' Out's log).
    let p0 = if kind == 6 {
        match vars.get(scope::PARAMETER, 0) {
            Some(v) => v.value,
            None => return PrimResult::Stop,
        }
    } else {
        0
    };
    let Some(cur) = vars.get(sc, data) else { return PrimResult::Stop };
    let cur = cur.value;
    let set = |vars: &mut ScriptVariables, id: i16| if vars.set(sc, data, id) { PrimResult::True } else { PrimResult::Stop };
    if kind == 5 {
        // The next neighbour by id; after the last, the target 0 and false.
        if let Some(id) = vars.world.neighbours.iter().map(|nb| nb.id).find(|&id| id > cur) {
            return set(vars, id);
        }
        vars.set(sc, data, 0);
        return PrimResult::False;
    }
    let w = &*vars.world;
    let order = w.order();
    if kind == 3 {
        // The next part of the target's multi-tile object, in the list's order, round from the
        // last part to the first; false if the target is not a part of one.
        let Some(t) = w.find(cur) else { return PrimResult::False };
        let leader = w.get(t).leader;
        if leader == 0 {
            return PrimResult::False;
        }
        let at = order.iter().position(|&k| k == t).unwrap_or(order.len());
        let next = (1..order.len()).map(|k| w.get(order[(at + k) % order.len()])).find(|x| x.leader == leader).map(Object::id);
        return match next {
            Some(id) => set(vars, id),
            None => PrimResult::False,
        };
    }
    let fits = |x: &Object| match kind {
        0 => true,
        1 => x.person,
        2 => !x.person,
        6 => x.data.get(59) == Some(&p0),
        _ => false,
    };
    if kind == 4 {
        // The next object of the GUID by id, not the list's order (the definition's own list,
        // which the original keeps in id order): 9,753 of 9,755 of the home log's steps, all of
        // the club's, the global init trees' walks (0x190-0x193) among them.
        let next = w.objects().filter(|x| x.guid == guid && x.id() > cur).map(Object::id).min();
        return match next {
            Some(id) => set(vars, id),
            // after the last, the target 0 too (the club's 2,464 steps of tree 4120 node 1)
            None => {
                vars.set(sc, data, 0);
                PrimResult::False
            }
        };
    }
    let start = if cur != 0 { order.iter().position(|&k| w.get(k).id() == cur).map_or(0, |k| k + 1) } else { 0 };
    match order[start.min(order.len())..].iter().map(|&k| w.get(k)).find(|x| fits(x)).map(Object::id) {
        Some(id) => set(vars, id),
        // after the last, false, and the target 0 for kinds 1 and 2 (as kinds 4 and 5): the
        // home log's 2,757 stack objects left set, 2,704 of them; kinds 0 and 6 keep their
        // target (the club's 9 steps from 30 in trees 8374 and 8194, its 16 of tree 4103 from 12)
        None => {
            if kind != 0 && kind != 6 {
                vars.set(sc, data, 0);
            }
            PrimResult::False
        }
    }
}

/// Idle (opcode 0, the PC's Sleep): the stack sleeps for as many of the tick's passes as the
/// frame's parameter (operand: its index) holds, yielding: passed over by the next N - 1 that
/// reach it, stepped on the N-th. Then the parameter is 0 and the node goes to its true target.
/// A missing parameter, or one of 0 or less: no sleep. Measured: all 2,810 regular sleeps of a
/// log with the pass's cursor (`docs/primitives.md`, "Idle").
pub fn idle(sleep: &mut Sleep, f: &mut Frame, n: &Node) -> PrimResult {
    let index = n.operand16(0) as i16;
    let Some(p) = usize::try_from(index).ok().filter(|&i| i < f.params.len()) else { return PrimResult::True };
    if sleep.woke() {
        sleep.wake();
        f.params[p] = 0;
        return PrimResult::True;
    }
    let ticks = f.params[p];
    if ticks <= 0 {
        return PrimResult::True;
    }
    sleep.sleep(ticks as u32 - 1);
    PrimResult::Yield
}

/// CallNamedTree (opcode 28): the tree whose name is a string (operand bytes 0-1 its STR#, byte 4
/// its index, counted from 1) of a file (bytes 2-3: 1 the global file, 2 the semi-global, 3 the
/// object's own; otherwise the calling tree's own kind), looked up by name in the stack
/// object's files (its own, its semi-global, the global file) and called on it: pushed as a
/// call, whose return takes the node's true or false way (Continue). No such string, or no tree
/// of that name: false. Measured: `docs/primitives.md`, "CallNamedTree".
pub fn call_named_tree(interp: &mut Interpreter, n: &Node, world: &World) -> PrimResult {
    let lib = world.library();
    let f = interp.stack().last().expect("a frame");
    let own = f.file;
    let semi = own.and_then(|o| lib.file(o).semi);
    let file = match field(n, 2) {
        1 => lib.global(),
        2 => semi,
        3 => own,
        _ => match f.tree.map(|t| tree_kind(lib.tree_at(t).id as u16)) {
            // the calling tree's own kind
            Some(TreeKind::Global) => lib.global(),
            Some(TreeKind::SemiGlobal) => semi,
            Some(_) => own,
            None => None,
        },
    };
    let Some(file) = file else { return PrimResult::False };
    let index = n.operand[4] as usize;
    let Some(name) = lib.file(file).strings.get(&(field(n, 0) as u16)).and_then(|t| t.get(index.wrapping_sub(1))).filter(|_| index >= 1) else {
        return PrimResult::False;
    };
    if name.is_empty() {
        return PrimResult::False;
    }
    // The tree of that name: the stack object's own file's, its semi-global's, the global file's.
    let so_file = world.find_object(f.stack_object).and_then(|o| o.file);
    for where_ in [so_file, so_file.and_then(|s| lib.file(s).semi), lib.global()].into_iter().flatten() {
        let wf = lib.file(where_);
        let Some(t) = wf.tree_by_name.get(name).and_then(|id| wf.trees.get(id)).copied() else { continue };
        if !interp.push(lib, t, &[]) {
            return PrimResult::Stop;
        }
        interp.top().file = Some(where_);
        return PrimResult::Continue;
    }
    PrimResult::False
}

/// IdleForInput (opcode 17) with nothing queued for the Sim: a count of the ticks to wait, in the
/// parameter that operand bytes 0-1 name. Above zero, it goes down by one and the node waits
/// (Yield); at zero, true. A parameter the frame does not have is the original's script error 8
/// (Stop). Read from the original's 80226378: this is its branch for a Sim with nothing to break
/// its wait (the vtable +0x240 test, Castaway's `cXPerson::ShouldInterrupt`, whose last test is
/// the queue's count); with something queued it delivers it (`idle_for_input`). Not modelled: the
/// flag the original sets and clears with it (bit 0x40 of +0x36 of the stack's owner), which
/// nothing in the logs reads.
pub fn idle_for_input_waiting(n: &Node, f: &mut Frame) -> PrimResult {
    let Some(count) = usize::try_from(field(n, 0)).ok().and_then(|p| f.params.get_mut(p)) else { return PrimResult::Stop };
    if *count == 0 {
        return PrimResult::True;
    }
    *count -= 1;
    PrimResult::Yield
}

/// IdleForInput (opcode 17), where a Sim waits in "Wait For Notify" (Global 281) for its next
/// interaction. With an entry queued: the engine's checks on the entry's target, as its
/// interaction table (TTAB) names them (the tree the table runs first, then the interaction's
/// guard; run by `Host::run_check`), then the entry's tree (the target's) pushed on the stack
/// with the target as stack object and zero parameters; the entry leaves the queue, and the
/// step goes on into it (Continue; this node stays, and the interaction's return comes back to
/// it). `docs/simulation.md`, "Where a Sim's interactions come in". Not modelled yet: nothing
/// queued (autonomy's, or a wait), a check that fails, a host that cannot run checks: Stop.
pub fn idle_for_input<'l, H: SimHost<'l>>(interp: &mut Interpreter, host: &mut H, me: ObjectKey) -> PrimResult {
    let world = host.world();
    let lib = world.library();
    let Some(&entry) = world.get(me).queue.first() else { return PrimResult::Stop };
    let Some(target) = world.find(entry.objects[1]) else { return PrimResult::Stop };
    if lib.global().is_none() {
        return PrimResult::Stop;
    }
    let t = world.get(target);
    let (tfile, tid, tperson, tdef) = (t.file, t.id(), t.person, world.definition(t));
    let Some(action) = lib.tree(tfile, entry.tree as u16) else { return PrimResult::Stop };
    let Some(tf) = tfile else { return PrimResult::Stop };
    // The checks, each as the target's file names it: the tree its interaction table names to
    // run first (the club's: Global 463, "Dream Sequence - Prefix Check Tree"), then the
    // interaction's guard, where the table has them. An entry the engine queued knows its place
    // in the target's table: that table's first tree and that interaction's guard. A saved one
    // is looked up by its tree, file-wide.
    let tab = lib.table_of(tfile, tperson, tdef);
    let checks: Vec<u16> = match (entry.interaction, tab.file, tab.table) {
        (Some(i), Some(f), Some(id)) if i < lib.table(f, id).len() => vec![tab.first, lib.table(f, id)[i].guard],
        _ => {
            let file = lib.file(tf);
            [&file.prefix_of, &file.guard_of].iter().filter_map(|m| m.get(&(entry.tree as u16)).copied()).collect()
        }
    };
    let defaults = interp.defaults().to_vec();
    for id in checks {
        if id == 0 {
            continue;
        }
        // As the object's own file names it.
        let ok = lib.tree(tfile, id).and_then(|t| host.run_check(t, tfile, tid, &defaults));
        if ok != Some(true) {
            return PrimResult::Stop;
        }
    }
    // the entry leaves the queue and is the Sim's current interaction (what an evaluation's join
    // check of the Sim reads: `autonomy::Candidate::join`)
    let o = host.world().get_mut(me);
    o.current = o.queue.remove(0);
    if !interp.push(lib, action, &[]) {
        return PrimResult::Stop;
    }
    interp.top().stack_object = tid;
    interp.top().file = tfile;
    PrimResult::Continue
}

/// A function as FindBestObjectForFunction (14) and RunFunctionalTree (20) name it (their operand's
/// first byte): the object datum that rates it (-1: none, 50 for all) and its entry in an object's
/// function table (OBJf). Read in the recomp (fn_8020CC8C, `docs/primitives.md`); the functions
/// not listed are not known.
pub fn function_entry(function: u8) -> Option<(i16, usize)> {
    Some(match function {
        0 => (0x1F, 0x12),
        1 => (0x20, 0x13),
        2 => (0x21, 0x14),
        3 => (0x24, 0x15),
        6 => (0x25, 0x18),
        7 => (0x26, 0x19),
        8 => (-1, 0x1A),
        10 => (0x3D, 0x0E),
        11 => (0x27, 0x1C),
        12 => (0x40, 0x10),
        13 => (0x41, 0x11),
        14 => (0x0F, 0x1D),
        _ => return None,
    })
}

/// RunFunctionalTree (opcode 20): the stack object's function (operand byte 0, `function_entry`)
/// run: its function table's action tree pushed with the Sim's temps as its parameters, the stack
/// object and its file the frame's; the node stays (Continue) and the tree's return takes its true
/// or false way. Measured on the home log: food_sg's 8234 node 3 (function 7, the table 47's 8199)
/// and 8206 node 11 (function 1, the stove 61's 8197), their parameters the temps. No such function
/// or object: Stop. (`docs/primitives.md`, "RunFunctionalTree".)
pub fn run_functional_tree<'l, H: SimHost<'l>>(interp: &mut Interpreter, host: &mut H, n: &Node) -> PrimResult {
    let world = host.world();
    let lib = world.library();
    let Some((_, entry)) = function_entry(n.operand[0]) else { return PrimResult::Stop };
    let so = interp.top().stack_object;
    let Some(o) = world.find(so) else { return PrimResult::Stop };
    let Some(tree) = world.function_tree(o, entry) else { return PrimResult::Stop };
    let file = world.get(o).file;
    let mut temps = [0i16; 4];
    let given = interp.defaults().len().min(temps.len());
    temps[..given].copy_from_slice(&interp.defaults()[..given]);
    if !interp.push(lib, tree, &temps[..given]) {
        return PrimResult::Stop;
    }
    interp.top().stack_object = so;
    interp.top().file = file;
    PrimResult::Continue
}

/// PushAction (opcode 13): queues an interaction for the Sim that is the stack object: the
/// interaction in place byte 0 of the target's interaction table (its file's TTAB), the target
/// the object whose id local byte 1 holds; true. Measured: the interactions the controller
/// pushes onto the club's Sims are the ones IdleForInput then delivers (`docs/simulation.md`).
/// Operand byte 2 the priority (1 maximum: ahead of everything queued; 2 ahead of saved
/// entries only; `SavedQueueEntry::rank`). Not modelled yet:
/// byte 3 (0x0a), a target whose file has several TTABs: Stop.
pub fn push_action<'l, H: SimHost<'l>>(interp: &mut Interpreter, host: &mut H, n: &Node) -> PrimResult {
    let top = interp.top_index();
    let target_id = interp.local(host, top, n.operand[1] as usize);
    let so = interp.stack()[top].stack_object;
    let world = host.world();
    let lib = world.library();
    let (Some(sim), Some(target)) = (world.find(so), world.find(target_id)) else { return PrimResult::Stop };
    // The target's own table (Library::table_of): its definition's, a person's semi-global's.
    let t = world.get(target);
    let tab = lib.table_of(t.file, t.person, world.definition(t));
    let (Some(f), Some(id)) = (tab.file, tab.table) else { return PrimResult::Stop };
    let Some(e) = lib.table(f, id).get(n.operand[0] as usize) else { return PrimResult::Stop };
    let entry = SavedQueueEntry {
        objects: [world.get(sim).id(), t.id(), 0],
        tree: e.action as i16,
        interaction: Some(n.operand[0] as usize),
        ..Default::default()
    };
    // operand byte 2 the priority: 1 (maximum) goes ahead of everything waiting (the club's
    // party controller's 4106 onto Sim 178, before its re-queued current entry), 2 behind the
    // other pushes but ahead of a saved entry (4119, the same); in its rank, after the others
    let rank = if n.operand[2] == 1 { 3 } else { 2 };
    let entry = SavedQueueEntry { rank, ..entry };
    let q = &mut world.get_mut(sim).queue;
    let at = q.iter().position(|e| e.rank < rank).unwrap_or(q.len());
    q.insert(at, entry);
    PrimResult::True
}

/// SetMotiveDelta (opcode 29): a running change of one of the Sim's (`vars.me`) motives. Its
/// operand (Castaway's SetMotiveDeltaParam): byte 0 the change's scope and field 4 its datum (a
/// change an hour), byte 1 the stop value's scope and field 6 its datum, byte 2 the motive, byte
/// 3 flags (1 clear all first; 2 and 4 an influence on the Sim or the stack object, not this,
/// `None`). The change a tick is the hour's / 1800 (30 ticks a second, a game hour a real
/// minute). Measured: the home log's one use, Sim 237 at the easel, fun +48 an hour to 50: its
/// 593 steps of +0.026667 the same (`docs/simulation.md`, "The motives").
pub fn set_motive_delta(n: &Node, vars: &mut ScriptVariables) -> Option<PrimResult> {
    let o = &n.operand;
    let flags = o[3];
    if flags & 6 != 0 {
        return None; // an influence: not this
    }
    let me = vars.me;
    if !vars.world.get(me).person {
        return Some(PrimResult::Stop);
    }
    if flags & 1 != 0 {
        vars.world.get_mut(me).motive_deltas.clear();
    }
    let rate = vars.get(o[0], field(n, 4));
    let stop = vars.get(o[1], field(n, 6));
    let (Some(rate), Some(stop)) = (rate, stop) else { return Some(PrimResult::Stop) };
    let d = MotiveDelta { motive: o[2] as i32, rate: rate.value as f32 / 1800.0, target: stop.value as f32 };
    // One change a motive: a new one takes the old one's place (the log has only one use).
    let deltas = &mut vars.world.get_mut(me).motive_deltas;
    match deltas.iter_mut().find(|x| x.motive == d.motive) {
        Some(x) => *x = d,
        None => deltas.push(d),
    }
    Some(PrimResult::True)
}
