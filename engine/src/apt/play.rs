//! Drives the player from an observer log: the game's movie loads and calls, in its frame
//! order, with the movie frames run between; and compares the commands the movies send with the
//! ones the game's log has.

use std::rc::Rc;

use super::Flash;
use super::player::{Command, Player};
use super::value::Value;
use crate::tree_log::Log;

/// How the game's frame counter maps to movie frames.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Movie frames per game frame.
    pub rate: f32,
    /// Whether a game frame's loads and calls come before its movie frames.
    pub calls_first: bool,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing { rate: 1.0, calls_first: false }
    }
}

/// What was run, against the log's commands.
pub struct Report {
    /// The commands in the log.
    pub total: usize,
    /// How many of them our commands have in the same order.
    pub same: usize,
    /// The first of the log's commands ours do not have: its index, the game frame.
    pub first_diff: Option<usize>,
    /// For each of the log's commands whether ours matched it.
    pub matched: Vec<bool>,
    /// For each matched command the game frame ours was sent at.
    pub ours_frame: Vec<u32>,
    /// Ours that matched none of the log's.
    pub extra: Vec<Command>,
}

/// The player run over a log's loads and calls up to game frame `to` (the whole log if `None`).
pub fn run(log: &Log, flashes: Vec<(String, Rc<Flash>)>, to: Option<u32>, timing: Timing) -> (Player, Vec<Command>) {
    let mut p = Player::new(flashes);
    // the values the game gives the movies
    p.set_extern("_platform", Value::str("GC"));
    p.set_extern("_lang", Value::str("en"));
    p.set_extern("_sku", Value::str("us"));
    p.set_extern("_enable_debug", Value::str("0"));
    p.set_reads(log.movie_reads.iter().map(|r| (r.frame, r.name.clone(), r.value.clone())));
    let last = log.movie_loads.iter().map(|l| l.frame).chain(log.movie_calls.iter().map(|c| c.frame)).chain(log.movie_commands.iter().map(|c| c.frame)).max().unwrap_or(0);
    let last = to.map_or(last, |t| t.min(last));
    let host = super::host::Host::for_log(log);
    let (mut li, mut ci) = (0, 0);
    let mut owed = 0.0f32;
    for f in 0..=last {
        p.frame = f;
        // The game loads a movie when a movie has asked for it, and its log does not say
        // whether a call of the same frame came before or after: a load is taken as soon as the
        // request is there.
        let loads = |p: &mut Player, li: &mut usize| {
            while *li < log.movie_loads.len() && log.movie_loads[*li].frame <= f && p.can_load(&log.movie_loads[*li].path) {
                p.load(&log.movie_loads[*li].path);
                *li += 1;
            }
        };
        let calls = |p: &mut Player, ci: &mut usize, li: &mut usize| {
            while *ci < log.movie_calls.len() && log.movie_calls[*ci].frame <= f {
                loads(p, li);
                let c = &log.movie_calls[*ci];
                if p.trace {
                    eprintln!("apt: game frame {f}: call {}({:?})", c.name, c.args.iter().map(|a| a.1.as_str()).collect::<Vec<_>>());
                }
                let args: Vec<Value> = c.args.iter().map(|(w, t)| if t.is_empty() { Value::Num(f64::from(*w as i32)) } else { Value::str(t) }).collect();
                if !p.call(&c.name, &args) {
                    p.note_stub(format!("call of {}: no such function", c.name));
                }
                *ci += 1;
            }
            loads(p, li);
        };
        if p.trace {
            eprintln!("apt: game frame {f}");
        }
        if timing.calls_first {
            calls(&mut p, &mut ci, &mut li);
        }
        owed += timing.rate;
        while owed >= 1.0 {
            owed -= 1.0;
            p.advance();
        }
        p.tick_loads();
        loads(&mut p, &mut li);
        if !timing.calls_first {
            calls(&mut p, &mut ci, &mut li);
        }
    }
    let mut sent = p.take_commands();
    sent.sort_by_key(|c| c.frame);
    let cmds = host.as_heard(sent);
    (p, cmds)
}

/// Aligns our commands with the log's (the longest run both have in order).
pub fn compare(log: &Log, ours: &[Command], to: Option<u32>) -> Report {
    // The game logs a command only once a screen has a handler for it; what the movies sent
    // before the log's first command (the wrapper's and the dialogs' start-up `_ready`s) went
    // nowhere.
    let first = log.movie_commands.iter().map(|c| c.frame).min().unwrap_or(0);
    let ours: Vec<Command> = ours.iter().filter(|c| c.frame >= first).cloned().collect();
    let ours = &ours[..];
    let theirs: Vec<(&str, &str)> = log.movie_commands.iter().filter(|c| to.is_none_or(|t| c.frame <= t)).map(|c| (c.command.as_str(), c.argument.as_str())).collect();
    let (n, m) = (theirs.len(), ours.len());
    fn key(c: &Command) -> (&str, &str) {
        (c.name.as_str(), c.argument.as_str())
    }
    let mut l = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if theirs[i] == key(&ours[j]) { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut matched = vec![false; n];
    let mut ours_frame = vec![0u32; n];
    let mut used = vec![false; m];
    while i < n && j < m {
        if theirs[i] == key(&ours[j]) {
            matched[i] = true;
            ours_frame[i] = ours[j].frame;
            used[j] = true;
            i += 1;
            j += 1;
        } else if l[i + 1][j] >= l[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    Report {
        total: n,
        same: matched.iter().filter(|&&b| b).count(),
        first_diff: matched.iter().position(|&b| !b),
        matched,
        ours_frame,
        extra: ours.iter().zip(&used).filter(|(_, u)| !**u).map(|(c, _)| c.clone()).collect(),
    }
}

