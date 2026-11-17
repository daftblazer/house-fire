
use crate::expression::Variables;
use crate::interp::{Frame};
use crate::tree_log::StepRec;
use crate::world::{ObjectKey, ScriptVariables, World};
use super::study::{env, has, atoi};
use super::counts::{Counts, Shown};

/// Brings the native world's variables to what the original's lookups found, counting those
/// that were already the same.
pub(super) fn sync_variables(world: &mut World, counts: &mut Counts, shown: &mut Shown, me: ObjectKey, f: &mut Frame, s: &StepRec) {
    let mut vars = ScriptVariables::new(world, me, f);
    for v in &s.lookups {
        if v.flags & 1 == 0 {
            continue; // the lookup found nothing
        }
        let Some(got) = vars.get(v.scope as u8, v.data) else {
            if env("REPLAY_SHOW_SCOPE").is_some_and(|e| atoi(&e) == v.scope as i64) && shown.next("scope", 40) {
                let so = s.stack_object;
                println!(
                    "scope {} datum {} (not modelled): log {} (flags {}, object {:08x} ptr {:08x}, offset {:#x}); me {}, stack object {so}, tree {}:{}",
                    v.scope,
                    v.data,
                    v.value,
                    v.flags,
                    v.object,
                    v.ptr,
                    v.ptr.wrapping_sub(v.object) as i32,
                    vars.world.get(vars.me).id(),
                    s.tree,
                    s.node
                );
                if let Some(o) = vars.world.find_object(so) {
                    let pd = o.person_data.get(v.data as usize).copied();
                    let d = o.data.get(v.data as usize).copied();
                    let room = vars.world.walls.room(o.x16 >> 4, o.y16 >> 4);
                    println!("   so {so}: data[{}] {d:?} person_data {pd:?} tile ({},{},{}) room {room} file {:?}", v.data, o.x16 >> 4, o.y16 >> 4, o.level, o.file.map(|f| vars.world.library().file(f).path.clone()));
                }
            }
            note!(counts, "variables: scope {}: not modelled", v.scope);
            continue;
        };
        if v.flags & 4 != 0 {
            let lf = f32::from_bits(v.fvalue);
            if got.f == Some(lf) {
                note!(counts, "variables: scope {}: the same natively", v.scope);
                continue;
            }
            note!(counts, "variables: scope {}: different (set to the log's)", v.scope);
            vars.set_float(v.scope as u8, v.data, lf);
            continue;
        }
        if got.value == v.value {
            if env("REPLAY_SHOW_SCOPE_SAME").is_some_and(|e| atoi(&e) == v.scope as i64) && shown.next("scope same", 20) {
                println!("scope {} datum {}: the same, {}", v.scope, v.data, v.value);
            }
            note!(counts, "variables: scope {}: the same natively", v.scope);
            continue;
        }
        if env("REPLAY_SHOW_SCOPE").is_some_and(|e| atoi(&e) == v.scope as i64) && shown.next("scope", env("REPLAY_SHOW_N").map_or(10, |n| atoi(&n) as i32)) {
            println!(
                "scope {} datum {}: native {}, log {}; the step's stack object {} -> {}, tree {}:{} (me {} {:?}, tick {}; the original's offset {:#x}; my turns {})",
                v.scope,
                v.data,
                got.value,
                v.value,
                s.stack_object,
                s.stack_object_after,
                s.tree,
                s.node,
                vars.world.get(me).id(),
                vars.world.get(me).file.map(|f| vars.world.library().file(f).path.rsplit('/').next().unwrap_or("").to_string()),
                s.tick,
                v.ptr.wrapping_sub(v.object) as i32,
                vars.world.get(me).turns
            );
        }
        // a local this frame has not written: zero here, the original's left-over memory there
        // (the listed deliberate difference, plan.md), counted apart
        if v.scope == 25 && (0..64).contains(&v.data) && (vars.frame.unset >> v.data & 1) != 0 {
            note!(counts, "variables: scope 25: an unwritten local, the original's left over (zero here, deliberately)");
            // study REPLAY_ZERO_LOCALS: keep the engine's zero, to see what the scripts do with it
            vars.set(v.scope as u8, v.data, if has("REPLAY_ZERO_LOCALS") { 0 } else { v.value });
            continue;
        }
        note!(counts, "variables: scope {}: different (set to the log's)", v.scope);
        vars.set(v.scope as u8, v.data, v.value);
    }
}
