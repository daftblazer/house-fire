//! What the game gives its screens' movies, and takes from them, that the observer does not
//! record: the numbers it sets in `extern` before it calls a movie. They are inferred from the
//! commands the movies sent back (`docs/assets.md`, "The player"): none of it is verified
//! against the game, only against what it makes the movies say.

use std::collections::{BTreeMap, BTreeSet};

use super::player::{Command, Player};
use super::value::Value;
use crate::tree_log::Log;

/// Values set at a game frame by hand: (frame, `extern` member, number). Each was fitted to what
/// the movies sent in answer (the menu's item counts to its buttons' places, a dialog's size and
/// place to its title bar's).
const FITTED: &[(u32, &str, f64)] = &[
    (738, "number_big_items", 1.0),
    (738, "number_small_items", 3.0),
    (738, "current_item", 0.0),
    (738, "menu_disable_ID", -1.0),
    (754, "number_big_items", 3.0),
    (1206, "current_load", 100.0),
    (1099, "dialog_box_width", 0.375),
    (1099, "dialog_box_height", 0.55),
    (1099, "dialog_x", 0.3125),
    (1099, "dialog_y", 0.18),
    (1099, "dialog_title_bar_width", 0.3),
    (1099, "UI_button_width", 0.056_406_25),
    (1099, "button_width", 0.1),
    (1099, "number_of_buttons", 2.0),
    (1099, "is_critical_dialog", 0.0),
    (1248, "number_of_buttons", 1.0),
    (1248, "dialog_box_height", 0.418_405),
    (907, "mu_dialog_width", 0.375),
    (907, "mu_dialog_height", 0.3),
    (907, "mu_dialog_x", 0.3125),
    (907, "mu_dialog_y", 0.145_089_29),
];

/// What the game does to the values at game frames: (frame, name, the number or `None` to remove
/// it).
pub struct Host {
    sets: Vec<(u32, String, Option<f64>)>,
    clears: Vec<(u32, String)>,
    /// How many screens heard the commands of a game frame (frames not listed: one).
    listeners: BTreeMap<u32, usize>,
}

impl Host {
    /// The game's side of a log: the places it gave the buttons. A button's place
    /// (`<context>_button_<name>_x` and `_y`, as fractions of the screen) is set before the
    /// `createButton` or `resetButton` call that reads it, and is gone once the button is
    /// destroyed. Neither is recorded; the place is taken from the commands the button sent
    /// back (the last value of the frame) and set at the last `createButton` or `resetButton` of
    /// the button before them.
    pub fn for_log(log: &Log) -> Host {
        // (frame, button name with its context's prefix, destroyed)
        let mut calls: Vec<(u32, String, bool)> = Vec::new();
        let mut ctx: Vec<String> = Vec::new();
        for c in &log.movie_calls {
            let arg = c.args.first().map_or("", |a| a.1.as_str());
            match c.name.as_str() {
                "setButtonContext" => ctx.push(arg.to_string()),
                "resetButtonContext" => {
                    ctx.pop();
                }
                "createButton" | "resetButton" | "destroyButton" => {
                    let prefix = ctx.last().cloned().unwrap_or_default();
                    calls.push((c.frame, format!("{prefix}_button_{arg}"), c.name == "destroyButton"));
                }
                _ => {}
            }
        }
        // Every screen the game has up hears a command a movie sends, and the observer logs each
        // hearing: the commands of a frame come once for each screen that logs any in it.
        let mut screens: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for c in &log.movie_commands {
            screens.entry(c.frame).or_default().insert(c.screen);
        }
        let listeners = screens.into_iter().map(|(f, s)| (f, s.len())).collect();
        let mut host = Host { sets: Vec::new(), clears: Vec::new(), listeners };
        for (i, c) in log.movie_commands.iter().enumerate() {
            let Some((base, v)) = [c.command.strip_suffix("_x"), c.command.strip_suffix("_y")].into_iter().flatten().next().zip(c.argument.parse::<f64>().ok()) else { continue };
            // a button taken off the screen reports -512 pixels: that is no place it was given
            if (v * 640.0 + 512.0).abs() < 0.01 || (v * 448.0 + 512.0).abs() < 0.01 {
                continue;
            }
            // only the last command of this name in the frame has the value
            let later = log.movie_commands[i + 1..].iter().take_while(|o| o.frame == c.frame).any(|o| o.command == c.command);
            if later {
                continue;
            }
            if let Some((at, ..)) = calls.iter().rfind(|(fr, n, gone)| *fr <= c.frame && n == base && !gone) {
                host.sets.push((*at, c.command.clone(), Some(v)));
            }
        }
        for (fr, n, gone) in &calls {
            if *gone {
                host.clears.push((*fr, format!("{n}_x")));
                host.clears.push((*fr, format!("{n}_y")));
            }
        }
        host
    }

    /// What the game sets before it calls the movies in game frame `f`.
    pub fn before_calls(&self, p: &mut Player, f: u32) {
        for (at, name, v) in FITTED {
            if *at == f {
                p.set_extern(name, Value::Num(*v));
            }
        }
        for (at, name, v) in &self.sets {
            if *at == f {
                p.set_extern(name, v.map_or(Value::Undefined, Value::Num));
            }
        }
    }

    /// The commands as the game's observer logs them: each one once for every screen that heard it.
    pub fn as_heard(&self, commands: Vec<Command>) -> Vec<Command> {
        commands.into_iter().flat_map(|c| std::iter::repeat_n(c.clone(), self.listeners.get(&c.frame).copied().unwrap_or(1))).collect()
    }

    /// What goes once the frame's calls are done.
    pub fn after_calls(&self, p: &mut Player, f: u32) {
        for (at, name) in &self.clears {
            if *at == f {
                p.set_extern(name, Value::Undefined);
            }
        }
    }
}
