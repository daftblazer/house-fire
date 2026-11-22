//! The pause screen's feed against the log's (LOCKSTEP_PAUSE=1, with the run's --card and
//! --lot): the engine's `apt::ui::Ui` (the movies and the game's side around them) given the
//! pad as the log has it ('0x81' records, the buttons held each game frame) and the world as the
//! replay runs it, against what the game did in each frame from the first Start to the last
//! `hidePause`: the movies it loaded, the calls it made (name and arguments), the commands the
//! movies sent back, each read's answer (by name) and each text line's string and place (to
//! 1e-4 of the screen). Prints `pause: same (0 differences)` or `pause: DIFFERENT (N
//! differences, first at frame F: ...)` and the differences by kind.
//!
//! The log is used only for the pad, the frames' ticks (the presentation's clock) and the
//! game's speed setting before the pause. LOCKSTEP_PAUSE_SHOW=N prints the first N
//! differences (default 6).

use crate::*;
use housefire::apt::pause::Game;
use housefire::apt::ui::{Feed, Ui};
use housefire::hud::{Hud, Measure, PlayerView, Speed};
use housefire::replay::{Replay, RunWith, Start};
use housefire::shader::name_hash;
use housefire::tree_log::Log;

/// What the log has of one game frame.
#[derive(Default)]
struct Frame {
    loads: Vec<String>,
    calls: Vec<(String, Vec<String>)>,
    reads: Vec<(String, String)>,
    commands: Vec<(String, String)>,
    texts: Vec<(String, f32, f32)>,
}

#[derive(Default)]
struct Tally {
    loads: u32,
    calls: u32,
    commands: u32,
    reads: u32,
    strings: u32,
    places: u32,
    first: Option<String>,
    shown: usize,
    limit: usize,
}

impl Tally {
    fn miss(&mut self, what: String) {
        if self.first.is_none() {
            self.first = Some(what.clone());
        }
        if self.shown < self.limit {
            self.shown += 1;
            println!("   {what}");
        }
    }
}

/// A movie's name from a loaded path (`.../p_goals.apt` is `p_goals`).
fn stem(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    base.split('.').next().unwrap_or(base).to_string()
}

pub fn pause(log: &Log, d: &Disc, lib: &Library, card: Option<&str>, lot_index: i64) {
    let Some(&start_at) = log.pads.iter().find(|p| p.buttons & 0x1000 != 0 && p.tick > 0).map(|p| &p.frame) else {
        println!("pause: no Start in the log's pad records");
        return;
    };
    let Some(end) = log.movie_calls.iter().filter(|c| c.name == "hidePause").map(|c| c.frame).max() else {
        println!("pause: no hidePause in the log");
        return;
    };
    let Ok(start) = Start::read(d, card, lot_index) else { return };
    let house = std::env::var("REPLAY_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let catalog = housefire::anim::Catalog::read(d, house);
    let Some(mut replay) = Replay::new(log, lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: Some(catalog) }) else { return };
    let fonts = housefire::apt::read_fonts(&d.read("fonts.arc").unwrap_or_default());
    let Some(font) = fonts.into_iter().find(|(n, _)| name_hash(n) == 0x67f664fb).and_then(|(_, f)| f.ok()) else {
        println!("pause: no font");
        return;
    };
    let measure = Measure { font: &font };
    let flashes: Vec<(String, std::rc::Rc<housefire::apt::Flash>)> = housefire::apt::read_flashes(&d.read("flashes.arc").unwrap_or_default()).into_iter().filter_map(|(n, r)| r.ok().map(|f| (n, std::rc::Rc::new(f)))).collect();

    // the log's frames: their ticks, and what the game did in those of the pause
    let mut tick_of: BTreeMap<u32, u32> = BTreeMap::new();
    let mut note = |frame: u32, tick: u32| {
        let t = tick_of.entry(frame).or_default();
        *t = (*t).max(tick);
    };
    log.movie_calls.iter().filter(|c| c.tick > 0).for_each(|c| note(c.frame, c.tick));
    log.movie_reads.iter().filter(|r| r.tick > 0).for_each(|r| note(r.frame, r.tick));
    log.text_draws.iter().filter(|t| t.tick > 0).for_each(|t| note(t.frame, t.tick));
    let within = |f: u32| (start_at..=end).contains(&f);
    let mut frames: BTreeMap<u32, Frame> = (start_at..=end).map(|n| (n, Frame::default())).collect();
    for l in log.movie_loads.iter().filter(|l| within(l.frame)) {
        frames.get_mut(&l.frame).unwrap().loads.push(stem(&l.path));
    }
    for c in log.movie_calls.iter().filter(|c| within(c.frame)) {
        frames.get_mut(&c.frame).unwrap().calls.push((c.name.clone(), c.args.iter().map(|a| a.1.clone()).collect()));
    }
    for r in log.movie_reads.iter().filter(|r| within(r.frame)) {
        frames.get_mut(&r.frame).unwrap().reads.push((r.name.clone(), r.value.clone()));
    }
    // (each command arrives at both screens' handlers; the HUD's is taken)
    for c in log.movie_commands.iter().filter(|c| within(c.frame) && c.handler == 0x8008d190) {
        frames.get_mut(&c.frame).unwrap().commands.push((c.command.clone(), c.argument.clone()));
    }
    for t in log.text_draws.iter().filter(|t| within(t.frame)) {
        frames.get_mut(&t.frame).unwrap().texts.push((t.text.clone(), t.x, t.y));
    }

    // the interface from the lot's start, as ui-check --native runs it
    let first_call = |name: &str, from: u32| log.movie_calls.iter().find(|c| c.name == name && c.frame >= from).map(|c| c.frame);
    let Some(f0) = log.movie_calls.iter().find(|c| c.name == "loadFunctions" && c.args.first().is_some_and(|a| a.1 == "hud")).map(|c| c.frame) else {
        println!("pause: no loadFunctions(\"hud\") in the log");
        return;
    };
    let Some(show_at) = first_call("showHUD", f0) else { return };
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
    let mut ui = Ui::new(flashes);
    ui.set_game(Game::new(&d.read("quickdat.arc").unwrap_or_default(), lib, start.family_funds()));
    ui.set_frame(f0);
    let mut hud = Hud::new(1);
    let mut tally = Tally { limit: std::env::var("LOCKSTEP_PAUSE_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(6), ..Tally::default() };
    let mut tick = 0;
    let mut compared = 0;
    for n in f0..=end {
        if let Some((_, &t)) = tick_of.range(..=n).next_back() {
            tick = t;
        }
        while replay.tick() < tick && replay.step() {}
        let world = replay.world();
        let Some(sim) = world.objects().filter(|o| o.person && o.person_data.get(32) == Some(&0)).map(|o| o.id()).min() else { break };
        let Some(view) = PlayerView::of(world, sim) else { break };
        if n == show_at {
            ui.begin_play(&mut hud, &view);
        }
        if let Some(c) = log.cams.iter().take_while(|c| c.tick <= tick).last() {
            ui.set_cursor((c.cursor_at[0], c.cursor_at[1]));
        }
        let buttons = log.pads.iter().take_while(|p| p.frame <= n).last().map_or(0, |p| p.buttons);
        ui.record = within(n);
        let sent_before = ui.sent.len();
        if n >= start_at {
            ui.pad(buttons, &mut hud);
        }
        let speed = if ui.paused() { Speed::Paused } else { speed_at(tick) };
        let mut feed = Feed { hud: &mut hud, view: &view, speed, measure: &measure };
        let texts = ui.step(&mut feed);
        let answers = ui.answers(&feed);
        let (calls, loads) = ui.take_record();
        let Some(f) = frames.get(&n) else { continue };
        compared += 1;
        if loads != f.loads {
            tally.loads += loads.len().abs_diff(f.loads.len()).max(1) as u32;
            tally.miss(format!("frame {n} loads ours {loads:?}, the log's {:?}", f.loads));
        }
        let calls: Vec<(String, Vec<String>)> = calls.into_iter().filter(|c| !c.0.ends_with("Layer")).collect();
        let theirs: Vec<&(String, Vec<String>)> = f.calls.iter().filter(|c| !c.0.ends_with("Layer")).collect();
        if calls.iter().collect::<Vec<_>>() != theirs {
            tally.calls += calls.len().abs_diff(theirs.len()).max(1) as u32;
            tally.miss(format!("frame {n} calls ours {calls:?}, the log's {theirs:?}"));
        }
        let commands: Vec<(String, String)> = ui.sent[sent_before..].iter().map(|c| (c.name.clone(), c.argument.clone())).collect();
        if commands != f.commands {
            tally.commands += commands.len().abs_diff(f.commands.len()).max(1) as u32;
            tally.miss(format!("frame {n} commands ours {commands:?}, the log's {:?}", f.commands));
        }
        for (name, value) in &f.reads {
            let mine = answers.iter().find(|a| &a.0 == name).map(|a| a.1.as_str());
            if mine != Some(value.as_str()) {
                tally.reads += 1;
                tally.miss(format!("frame {n} read {name} ours {mine:?}, the log's {value:?}"));
            }
        }
        if texts.len() != f.texts.len() {
            tally.strings += texts.len().abs_diff(f.texts.len()) as u32;
            tally.miss(format!("frame {n} texts ours {:?}, the log's {:?}", texts.iter().map(|t| &t.text).collect::<Vec<_>>(), f.texts.iter().map(|t| &t.0).collect::<Vec<_>>()));
        }
        for (t, (text, x, y)) in texts.iter().zip(&f.texts) {
            if &t.text != text {
                tally.strings += 1;
                tally.miss(format!("frame {n} text ours {:?}, the log's {text:?}", t.text));
            } else if (t.x - x).abs() > 1e-4 || (t.y - y).abs() > 1e-4 {
                tally.places += 1;
                tally.miss(format!("frame {n} text {text:?} at ours ({:.5}, {:.5}), the log's ({x:.5}, {y:.5})", t.x, t.y));
            }
        }
    }
    let total = tally.loads + tally.calls + tally.commands + tally.reads + tally.strings + tally.places;
    if total == 0 {
        println!("pause: same (0 differences) in {compared} frames");
    } else {
        println!("pause: DIFFERENT ({total} differences, first at {}) in {compared} frames", tally.first.clone().unwrap_or_default());
        println!("pause:   loads {}, calls {}, commands {}, reads {}, text strings {}, text places {}", tally.loads, tally.calls, tally.commands, tally.reads, tally.strings, tally.places);
    }
}
