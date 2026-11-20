//! What the game does around its screens' movies that the player needs: which screens heard each
//! command. The values the game gives the movies are its own answers to their reads, logged by
//! the observer (`Player::set_reads`, `docs/assets.md`, "The player"): nothing is fitted here.

use std::collections::{BTreeMap, BTreeSet};

use super::player::Command;
use crate::tree_log::Log;

/// How many screens heard the commands of each game frame.
pub struct Host {
    /// How many screens heard the commands of a game frame (frames not listed: one).
    listeners: BTreeMap<u32, usize>,
}

impl Host {
    /// The game's side of a log. Every screen the game has up hears every command a movie sends,
    /// and the observer logs each hearing: the commands of a frame come once for each screen
    /// that logs any in it.
    pub fn for_log(log: &Log) -> Host {
        let mut screens: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for c in &log.movie_commands {
            screens.entry(c.frame).or_default().insert(c.screen);
        }
        Host { listeners: screens.into_iter().map(|(f, s)| (f, s.len())).collect() }
    }

    /// The commands as the game's observer logs them: each one once for every screen that heard it.
    pub fn as_heard(&self, commands: Vec<Command>) -> Vec<Command> {
        commands.into_iter().flat_map(|c| std::iter::repeat_n(c.clone(), self.listeners.get(&c.frame).copied().unwrap_or(1))).collect()
    }
}
