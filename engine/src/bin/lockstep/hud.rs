//! The HUD's and the dialog's feed against the log's (LOCKSTEP_HUD=1, with the run's --card and
//! --lot): the engine's `hud::Hud` for the player's Sim, given the world tick after tick as the
//! replay runs it, against what the game gave the Apt movies in each of its frames: the calls
//! (name and arguments), each read's answer, and each text line's string and place (to 1e-4 of
//! the screen). Prints `hud: same (0 differences)` or `hud: DIFFERENT (N differences, first at
//! tick T: ours ..., the log's ...)` and the differences by kind.
//!
//! The log is used for what the engine does not make: which frames the game had and in which
//! tick (the presentation's clock), the commands the movies sent back, and the game's speed
//! setting (the tracks' speeds: 5 is the fast setting, 10 the fastest). LOCKSTEP_HUD_SHOW=N
//! prints the first N differences (default 6); LOCKSTEP_HUD_EVENTS the dialogs that open.

use crate::*;
use housefire::hud::{Dialog, Hud, Measure, PlayerView, Speed};
use housefire::replay::{Replay, RunWith, Start};
use housefire::shader::name_hash;
use housefire::tree_log::Log;

/// What the log has of one game frame.
#[derive(Default)]
struct Frame {
    first_tick: u32,
    last_tick: u32,
    calls: Vec<(String, Vec<String>)>,
    reads: Vec<(u32, String, String)>,
    commands: Vec<(String, String)>,
    texts: Vec<(String, f32, f32)>,
}

impl Frame {
    fn at(&mut self, tick: u32) {
        self.first_tick = if self.first_tick == 0 { tick } else { self.first_tick.min(tick) };
        self.last_tick = self.last_tick.max(tick);
    }
}

/// The game's frames while a lot is played (the log's records after tick 0), by frame number.
fn frames_of(log: &Log) -> BTreeMap<u32, Frame> {
    let mut frames: BTreeMap<u32, Frame> = BTreeMap::new();
    for c in log.movie_calls.iter().filter(|c| c.tick > 0) {
        let f = frames.entry(c.frame).or_default();
        f.at(c.tick);
        f.calls.push((c.name.clone(), c.args.iter().map(|a| a.1.clone()).collect()));
    }
    for r in log.movie_reads.iter().filter(|r| r.tick > 0) {
        let f = frames.entry(r.frame).or_default();
        f.at(r.tick);
        f.reads.push((r.handler, r.name.clone(), r.value.clone()));
    }
    // (each command arrives at both screens' handlers; the HUD's is taken)
    for c in log.movie_commands.iter().filter(|c| c.tick > 0 && c.handler == 0x8008d190) {
        let f = frames.entry(c.frame).or_default();
        f.at(c.tick);
        f.commands.push((c.command.clone(), c.argument.clone()));
    }
    for t in log.text_draws.iter().filter(|t| t.tick > 0) {
        let f = frames.entry(t.frame).or_default();
        f.at(t.tick);
        f.texts.push((t.text.clone(), t.x, t.y));
    }
    frames
}

#[derive(Default)]
struct Tally {
    calls: u32,
    reads: u32,
    strings: u32,
    places: u32,
    first: Option<(u32, String)>,
    shown: usize,
    limit: usize,
}

impl Tally {
    fn miss(&mut self, tick: u32, what: String) {
        if self.first.is_none() {
            self.first = Some((tick, what.clone()));
        }
        if self.shown < self.limit {
            self.shown += 1;
            println!("   tick {tick}: {what}");
        }
    }
}

pub fn hud(log: &Log, d: &Disc, lib: &Library, ngh: &[u8], card: Option<&str>, lot_index: i64) {
    let Ok(start) = Start::read(d, card, lot_index) else { return };
    let house = std::env::var("REPLAY_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let catalog = housefire::anim::Catalog::read(d, house);
    let Some(mut replay) = Replay::new(log, lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: Some(catalog) }) else { return };
    let fonts = housefire::apt::read_fonts(&d.read("fonts.arc").unwrap_or_default());
    // (the font the HUD's text is drawn in: the hash of its name, as the text records have it)
    let Some(font) = fonts.into_iter().find(|(n, _)| name_hash(n) == 0x67f664fb).and_then(|(_, f)| f.ok()) else {
        println!("hud: no font");
        return;
    };
    let measure = Measure { font: &font };
    let names = housefire::figure::sim_names(ngh);
    let frames = frames_of(log);
    let speed_at = |tick: u32| {
        let s = log.tracks.iter().filter(|t| t.tick == tick).map(|t| t.speeds[1].abs()).fold(0.0, f32::max);
        if s == 10.0 {
            Speed::Fastest
        } else if s == 5.0 {
            Speed::Fast
        } else {
            Speed::Normal
        }
    };
    let mut tally = Tally { limit: std::env::var("LOCKSTEP_HUD_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(6), ..Tally::default() };
    let mut hud = Hud::new(1);
    let (mut dialog_open, mut first_frame, mut compared, mut last_speed) = (false, true, 0u32, Speed::Normal);
    let show_events = std::env::var_os("LOCKSTEP_HUD_EVENTS").is_some();
    for (&n, f) in &frames {
        while replay.tick() < f.last_tick && replay.step() {}
        let world = replay.world();
        // the player's Sim: the household's first (lowest id) resident
        let Some(sim) = world.objects().filter(|o| o.person && o.person_data.get(32) == Some(&0)).map(|o| o.id()).min() else { break };
        let Some(view) = PlayerView::of(world, sim) else { break };
        if first_frame {
            hud.show(&view);
        }
        // a Dialog primitive that opened (the node's operand has the text's place in its
        // object's STR# 301), and one that was answered
        if !dialog_open && world.dialog_object != 0 {
            let (object, operand) = (world.dialog_object, world.dialog_operand);
            let strings = world.find_object(object).and_then(|o| o.file).and_then(|file| lib.file(file).strings.get(&301));
            let strings: Vec<String> = strings.cloned().unwrap_or_default();
            // an object's name: a Sim's from the save, else its file's catalog name
            let name = |id: i16| match world.find_object(id) {
                Some(o) if o.person => o.person_data.get(31).and_then(|n| names.get(n)).cloned().unwrap_or_else(|| format!("object {id}")),
                Some(o) => o.file.and_then(|f| lib.file(f).catalog.values().next().and_then(|s| s.first()).cloned()).unwrap_or_else(|| format!("object {id}")),
                None => format!("object {id}"),
            };
            let dialog = Dialog::opened(&strings, operand, &world.dialog_locals, &name, &measure);
            if show_events {
                println!("hud: tick {} dialog from object {object}: {dialog:?} operand {operand:02x?}", replay.tick());
            }
            hud.show_dialog(dialog);
            dialog_open = true;
        }
        if dialog_open && world.dialog_object == 0 {
            hud.hide_dialog();
            dialog_open = false;
        }
        for (name, arg) in &f.commands {
            hud.command(name, arg);
        }
        // (the HUD takes the setting a frame after the animations do)
        let speed = std::mem::replace(&mut last_speed, speed_at(f.last_tick));
        let mut calls = hud.update(&view, speed, first_frame);
        first_frame = false;
        let (draw_calls, texts) = hud.draw(&view, &measure);
        calls.extend(draw_calls);
        compared += 1;
        let ours: Vec<(String, Vec<String>)> = calls.iter().map(|c| (c.name.to_string(), c.args.clone())).collect();
        if ours != f.calls {
            tally.calls += ours.len().abs_diff(f.calls.len()).max(1) as u32;
            tally.miss(f.first_tick, format!("frame {n} calls ours {ours:?}, the log's {:?}", f.calls));
        }
        for (handler, name, value) in &f.reads {
            let mine = if *handler == 0x8008bfdc { hud.answer(name, &view) } else { hud.dialog_answer(name) }.unwrap_or_default();
            if &mine != value {
                tally.reads += 1;
                tally.miss(f.first_tick, format!("frame {n} read {name} (handler {handler:x}) ours {mine:?}, the log's {value:?}"));
            }
        }
        if texts.len() != f.texts.len() {
            tally.strings += texts.len().abs_diff(f.texts.len()) as u32;
            tally.miss(f.first_tick, format!("frame {n} texts ours {:?}, the log's {:?}", texts.iter().map(|t| &t.text).collect::<Vec<_>>(), f.texts.iter().map(|t| &t.0).collect::<Vec<_>>()));
        }
        for (t, (text, x, y)) in texts.iter().zip(&f.texts) {
            if &t.text != text {
                tally.strings += 1;
                tally.miss(f.first_tick, format!("frame {n} text ours {:?}, the log's {text:?}", t.text));
            } else if (t.x - x).abs() > 1e-4 || (t.y - y).abs() > 1e-4 {
                tally.places += 1;
                tally.miss(f.first_tick, format!("frame {n} text {text:?} at ours ({:.5}, {:.5}), the log's ({x:.5}, {y:.5})", t.x, t.y));
            }
        }
    }
    let total = tally.calls + tally.reads + tally.strings + tally.places;
    if total == 0 {
        println!("hud: same (0 differences) in {compared} frames");
    } else {
        let (tick, what) = tally.first.clone().unwrap_or_default();
        println!("hud: DIFFERENT ({total} differences, first at tick {tick}: {what}) in {compared} frames");
        println!("hud:   calls {}, reads {}, text strings {}, text places {}", tally.calls, tally.reads, tally.strings, tally.places);
    }
}
