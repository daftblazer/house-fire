//! The club run natively, tick after tick, against a log of the recompiled Bustin' Out's
//! interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG) and the disc it ran.
//!
//! Usage: replay LOG ISO [--card CARD] [--lot N] [--bench]
//!
//! --card: the lot from a save on a memory card image (its first lot, Mom's house in the user's
//! save, unless --lot says), not the disc's club; the neighbours are the save's too.
//!
//! The world is the disc's club lot, loaded natively (the log starts as the club loads): each
//! object's saved tree stack, attributes, temps, data, and the Sims' motives and person data.
//! The simulator says whose turn it is each tick; the turns are compared with the log's. A tick
//! without the pass over the objects is a paused one, whose clock does not move, and steps
//! outside the pass are the engine's own (the pie menu's), not run (the log's 'Q' records). Each
//! stack is then stepped natively for as many steps as the log has for it in that tick, and
//! before each step its top frame (depth, tree, node) is compared with the log's. The native
//! state is kept from step to step and tick to tick: frames, sleep, the variables.
//!
//! The primitives the engine has run natively (the expression, Idle, SetToNext, ...) on the
//! native world; each variable they read is compared with what the original's lookup found
//! before it is used. The others are answered from the log: their result, and their frame's
//! parameters, locals and stack object after them.
//!
//! Where the native state differs from the log's it is counted and then set to the log's, so
//! that the run goes on: a frame (an interaction the engine pushed, which is not native yet), a
//! variable (one a primitive that is not native wrote). Objects that are not the lot's (the
//! household, made as the lot loads, and objects made later) are not run.
//!
//! Its output is the C++ tool's, line for line; of its studies (REPLAY_SHOW_...), the two that
//! showed native addresses show object keys.
//!
//! # Map of the submodules
//!
//! Kind (a): native logic that never reads the log (it belongs in the engine proper later).
//! Kind (b): comparison with the log, or answers taken from the log. Only `start` is wholly
//! kind (a); where a file mixes both, the kind-(a) functions are named.
//!
//! - `study`: the study switches (`REPLAY_*`), `atoi`, `file_name`. Neither kind (helpers).
//! - `counts`: `Counts`, `Bench` (timings), `Shown`. Neither kind (the report's bookkeeping).
//! - `start`: `Start` (the save's lots and neighbours), `RunWith`. Kind (a).
//! - `finder`: `TreeFinder`, a logged tree's place on the disc. Kind (b).
//! - `host`: `ReplayHost` and its `Host` impl, the primitives: native ones (the expression, Idle,
//!   SetToNext, objects, motives, the queue) with the log's answers for the rest. Mixed; the
//!   native arms of `Host::primitive` are kind (a) but are interleaved with the log's checks.
//! - `evaluation`: `find_best_object`, `evaluation` (autonomy's native choice, checked against
//!   the log's scores and candidates). Mixed; kind (a): `find_best_object`, `evaluation`'s
//!   scoring, with the log used to compare and to fall back.
//! - `animate`: `AnimReplay`, the Animate primitive through `anim::Animator`, its frames stepped
//!   by the log's 'k' records. Mixed; the compare, `animate_differs`, is kind (b).
//! - `variables`: `sync_variables`, the native variables set to the lookups the log found.
//!   Kind (b).
//! - `stacks`: which object each logged stack is (`stack_ids`, `place_quiet_stacks`: kind (b)),
//!   and `top_node` (kind (a)).
//! - `walks`: the route followers' headings (`turn_followers`: kind (b), from the log's 'Z'
//!   records), `native_plan_points` (the planner on a plan's recorded inputs) and `leg_at`
//!   (kind (a)).
//! - `run`: `Replay`, `run`; `Replay::new` loads the lot and builds the simulator (kind (a)) and
//!   indexes the log (kind (b)); the accessors (`world`, `sim_plays`, `tick`) are kind (a).
//! - `step`: `Replay::step`, one tick: the simulator's turns, each stack stepped natively as
//!   often as the log has steps for it, checks and corrections. Kind (b), around native
//!   stepping.
//! - `finish`: `Replay::finish`, the checks over the whole log. Kind (b).
//! - `routing`: the routing pass (`routing`): the original's plans and walks against the native
//!   planner and walker, and the native returns and places the host uses. Mixed; the calls into
//!   `crate::routing` and `crate::planner` are kind (a), the pass itself is one function.
//! - `sound`: which sound each PlaySound request starts, against the log's plays. Kind (b), with
//!   the native `ask`.
//! - `clock`: `check_time_of_day`, the light's clock and sun against the '3' records. Kind (b).


/// A count whose kind is formatted, only when counting.
macro_rules! note {
    ($counts:expr, $($arg:tt)*) => {
        if !$counts.quiet {
            $counts.note(&format!($($arg)*));
        }
    };
}

mod animate;
mod clock;
mod counts;
mod drive;
mod evaluation;
mod finder;
mod finish;
mod host;
mod routing;
mod run;
pub mod sound;
mod stacks;
mod start;
mod step;
mod study;
mod variables;
mod walks;

pub use counts::{Bench, Counts};
pub use drive::{DriveReport, Parting, drive};
pub use run::{Replay, run};
pub use start::{RunWith, Start};
pub use study::{atoi, env, has, switches};
