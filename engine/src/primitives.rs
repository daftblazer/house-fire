//! Primitives other than the expression (`expression`), each on the world (`world`).

use crate::bhav::Node;
use crate::expression::Variables;
use crate::iff::Order;
use crate::interp::{Frame, PrimResult, Sleep};
use crate::world::{Object, PresentationEvent, ScriptVariables, World, scope};

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
pub fn set_to_next(n: &Node, vars: &mut ScriptVariables) -> PrimResult {
    let o = &n.operand;
    let kind = o[4] & 0x7F;
    let (sc, data) = if o[4] & 0x80 != 0 { (o[5], o[7] as i16) } else { (scope::STACK_OBJECT, 0) };
    let guid = operand_guid(n);
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
        4 => x.guid == guid,
        6 => x.data.get(59) == Some(&p0),
        _ => false,
    };
    let start = if cur != 0 { order.iter().position(|&k| w.get(k).id() == cur).map_or(0, |k| k + 1) } else { 0 };
    match order[start.min(order.len())..].iter().map(|&k| w.get(k)).find(|x| fits(x)).map(Object::id) {
        Some(id) => set(vars, id),
        None => PrimResult::False,
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
