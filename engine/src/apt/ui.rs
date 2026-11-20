//! The game's interface run from the engine's own feed: the HUD, the dialog and the pause
//! screen as the Apt movies draw them, with no log. The movies are the `Player`'s; what the game
//! gives and does around them is `hud::Hud` (the calls it makes, the answers it gives their
//! reads, the lines of text it draws) and what this module adds for the screens the HUD does
//! not own: the pause screen's calls, the interaction menu's pills and the keys the player
//! presses (`docs/assets.md`, "In the sandbox (U4)").
//!
//! This is the presentation's side: it reads the simulation's world through a `PlayerView` and
//! gives back only what to draw; nothing here changes the world.

use std::rc::Rc;

use super::Flash;
use super::player::{Command, Player};
use super::value::Value;
use crate::hud::{Call, Hud, Measure, PlayerView, SCREEN, Speed, Text, line};

/// The keys the wrapper's `keys` clip knows (`Key.getCode`, read from its actions): the
/// arrows, A, B and the triangle.
pub mod keys {
    pub const LEFT: u32 = 37;
    pub const UP: u32 = 38;
    pub const RIGHT: u32 = 39;
    pub const DOWN: u32 = 40;
    pub const A: u32 = 88;
    pub const B: u32 = 79;
    pub const TRIANGLE: u32 = 84;
}

/// The buttons the pause screen makes and hides (`drawButton(name, "-1", "-1", ".1")`, 0x800A7614
/// and the seven after it), in the game's order.
const PAUSE_BUTTONS: [&str; 8] = ["accept", "decline", "walls", "alt2", "flipper1", "flipper2", "rotate", "background"];

/// The pause screen's two visible buttons: where the game puts them (fractions of the screen,
/// `drawButton` at 0x800A5018 and 0x800A504C) and their labels.
const PAUSE_ACCEPT: (&str, f32, f32) = ("SELECT", 0.4, 0.905);
const PAUSE_DECLINE: (&str, f32, f32) = ("BACK", 0.7, 0.905);

/// A button's label sits this far right of the button's place (2 pixels: the face of the
/// "SELECT" and "BACK" lines is centred on 0.403125 and 0.703125 for the buttons at 0.4 and 0.7).
const BUTTON_LABEL_SHIFT: f32 = 2.0 / SCREEN.0;

/// What the game measured of the pause screen's own strings, as the log has it: the line's
/// width in pixels (twice the distance from its place to the point it is aligned at) and, for the
/// buttons' labels, the width `drawButton` was given (a fraction of the screen). The font's
/// glyph columns do not give these (the advance is not the columns, `docs/assets.md`), and the
/// strings are fixed, so they are the log's own numbers (the pause screen of ui-menu.log).
const PAUSE_WIDTHS: [(&str, f32, f32); 3] = [("GOALS", 65.66, 0.0), ("SELECT", 63.9, 0.093525), ("BACK", 49.0, 0.070360)];

/// The pause screen's title ("GOALS" for the first section): centred on the middle, its top at
/// this fraction of the screen (the log's line, `0.1861`; the movie's title bar place is not
/// reported).
const PAUSE_TITLE_Y: f32 = 0.1861;

/// The pause screen's font is the HUD's at 14 where the HUD's is 13: its lines are 14/13 of the
/// HUD's (the "SELECT" line's height 17.2 pixels against 15.9).
const PAUSE_SIZE_RATIO: f32 = 14.0 / 13.0;

/// The game frames the pause screen's two buttons come after the movie's `draw_main` (2064 and
/// 2066 in the log).
const BUTTON_DELAY: u32 = 2;

/// How the pause screen stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pause {
    Closed,
    /// Open: the frame its buttons are drawn at (`draw_main` was heard), once heard.
    Open { buttons_at: Option<u32> },
}

/// What the game gives for one presentation frame.
pub struct Feed<'a> {
    pub hud: &'a mut Hud,
    pub view: &'a PlayerView,
    pub speed: Speed,
    pub measure: &'a Measure<'a>,
}

/// The interface of one player.
pub struct Ui {
    player: Player,
    frame: u32,
    first: bool,
    /// The HUD was shown and its `showHUD` is still to be called.
    announce: bool,
    pause: Pause,
    /// A call the pause screen's open or close waits to make.
    calls: Vec<Call>,
    /// Every command the movies sent, for a check against the log's.
    pub sent: Vec<Command>,
}

fn args(a: &[&str]) -> Vec<Value> {
    a.iter().map(|s| Value::str(s)).collect()
}

impl Ui {
    /// The interface with the wrapper loaded and the HUD's functions asked for (the lot's start,
    /// `loadFunctions("hud")` at 0x801066D8).
    pub fn new(flashes: Vec<(String, Rc<Flash>)>) -> Ui {
        let mut player = Player::new(flashes);
        player.set_extern("_platform", Value::str("GC"));
        player.set_extern("_lang", Value::str("en"));
        player.set_extern("_sku", Value::str("us"));
        player.set_extern("_enable_debug", Value::str("0"));
        Ui { player, frame: 0, first: true, announce: false, pause: Pause::Closed, calls: Vec::new(), sent: Vec::new() }
    }

    /// The game's frame the interface is at (the next to be run).
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Sets the game's frame number (a check starts the interface at the log's frame).
    pub fn set_frame(&mut self, frame: u32) {
        self.frame = frame;
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn player_mut(&mut self) -> &mut Player {
        &mut self.player
    }

    /// Whether the pause screen is up.
    pub fn paused(&self) -> bool {
        self.pause != Pause::Closed
    }

    /// A key goes down and up (the pad or keyboard, as the game passes it to the movies).
    pub fn press(&mut self, code: u32) {
        self.player.key(code, true);
        self.player.key(code, false);
    }

    /// Play begins: the HUD is shown (`showHUD` and the player mode on the next frame).
    pub fn begin_play(&mut self, hud: &mut Hud, view: &PlayerView) {
        hud.show(view);
        self.announce = true;
    }

    /// The pause screen opens (Start): the HUD goes, the pause movie loads with its buttons
    /// (`hideHUD`, `showPause`, the buttons made hidden: 0x800A7594 to 0x800A7764).
    pub fn open_pause(&mut self, hud: &mut Hud) {
        if self.paused() {
            return;
        }
        hud.pause();
        self.calls.push(Call { name: "hideHUD", args: Vec::new() });
        self.calls.push(Call { name: "showPause", args: Vec::new() });
        self.calls.push(Call { name: "setButtonContext", args: vec!["pause".into()] });
        for b in PAUSE_BUTTONS {
            self.calls.push(Call { name: "drawButton", args: vec![b.into(), "-1".into(), "-1".into(), ".1".into()] });
        }
        self.pause = Pause::Open { buttons_at: None };
    }

    /// The pause screen closes (B): its buttons destroyed, the HUD back (0x800A77F0 to 0x800A7960).
    pub fn close_pause(&mut self, hud: &mut Hud, view: &PlayerView) {
        if !self.paused() {
            return;
        }
        self.calls.push(Call { name: "hidePause", args: Vec::new() });
        self.calls.push(Call { name: "setButtonContext", args: vec!["pause".into()] });
        for b in PAUSE_BUTTONS {
            self.calls.push(Call { name: "destroyButton", args: vec![b.into()] });
        }
        self.calls.push(Call { name: "resetButtonContext", args: Vec::new() });
        hud.resume(view);
        self.pause = Pause::Closed;
    }

    /// The answers the game gives the movies' reads this frame.
    fn answers(&self, f: &Feed) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for name in ["om", "ma", "ra", "ct", "two_player"] {
            if let Some(v) = f.hud.answer(name, f.view) {
                out.push((name.to_string(), v));
            }
        }
        for name in ["number_of_buttons", "is_critical_dialog", "dialog_x", "dialog_y", "dialog_box_width", "dialog_box_height", "dialog_title_bar_width", "UI_button_width"] {
            if let Some(v) = f.hud.dialog_answer(name, f.measure) {
                out.push((name.to_string(), v));
            }
        }
        if self.paused() {
            // the pause movie's reads (0x800A60D4): the mode of play, whether the lot can be
            // built on, and the title's width
            out.push(("pause_mode".into(), "resident".into()));
            out.push(("pause_enable_buy_build".into(), "true".into()));
            out.push(("title_bar_width".into(), format!("{:.6}", pause_width(f.measure, PAUSE_TITLE).0 / SCREEN.0)));
        }
        out
    }

    /// Loads the movies that asked for it (the game loads what a movie's `loadMovie` names).
    fn load_pending(&mut self) {
        for name in self.player.pending_loads() {
            self.player.load(&name);
        }
    }

    fn make(&mut self, c: &Call) {
        let a: Vec<Value> = c.args.iter().map(|s| Value::str(s)).collect();
        if self.player.trace {
            eprintln!("ui: frame {} call {}({:?})", self.frame, c.name, c.args);
        }
        if !self.player.call(&c.name, &a) {
            self.player.note_stub(format!("call of {}: no such function", c.name));
        }
    }

    /// One presentation frame: the movies run a frame, load what they asked for, the game hears
    /// what they sent and makes its calls; the lines of text it draws are given back.
    pub fn step(&mut self, f: &mut Feed) -> Vec<Text> {
        if self.first {
            self.first = false;
            self.player.frame = self.frame;
            self.player.load("wrapper");
            // The dialog screen's button, made as its constructor does (0x800EDC3C to 0x800EDCC8):
            // `createButton("accept", "center")` leaves `orientation` set for every button the
            // wrapper makes after it, the dialog's own among them (centred on its place).
            self.player.call("setButtonContext", &args(&["mu"]));
            for b in ["accept", "decline", "alt1", "z"] {
                self.player.call("createButton", &args(&[b]));
                self.player.call("hideButton", &args(&[b]));
            }
            self.player.call("resetButtonContext", &[]);
            self.player.call("setButtonContext", &args(&["dlog"]));
            self.player.call("createButton", &args(&["accept", "center"]));
            self.player.call("hideButton", &args(&["accept"]));
            self.player.call("resetButtonContext", &[]);
            self.player.call("loadFunctions", &args(&["hud"]));
        }
        self.player.frame = self.frame;
        let answers = self.answers(f);
        self.player.set_answers(answers);
        self.player.advance();
        self.player.tick_loads();
        self.load_pending();
        for c in self.player.take_commands() {
            f.hud.command(&c.name, &c.argument);
            if c.name == "draw_main"
                && let Pause::Open { buttons_at: at @ None } = &mut self.pause
            {
                *at = Some(self.frame + BUTTON_DELAY);
            }
            self.sent.push(c);
        }
        let mut calls = std::mem::take(&mut self.calls);
        calls.extend(f.hud.update(f.view, f.speed, std::mem::take(&mut self.announce)));
        let (draw, mut texts) = f.hud.draw(f.view, f.measure);
        calls.extend(draw);
        if let Pause::Open { buttons_at: Some(at) } = self.pause
            && at == self.frame
        {
            for (label, x, y) in [PAUSE_ACCEPT, PAUSE_DECLINE] {
                let w = pause_width(f.measure, label).1;
                calls.push(Call { name: "drawButton", args: vec![if label == "SELECT" { "accept" } else { "decline" }.into(), format!("{x:.6}"), format!("{y:.6}"), format!("{w:.6}")] });
            }
        }
        for c in &calls {
            self.make(c);
        }
        if let Pause::Open { buttons_at: Some(at) } = self.pause
            && at <= self.frame
        {
            self.pause_texts(f, &mut texts);
        }
        self.frame += 1;
        texts
    }

    /// The pause screen's own lines: the title, the two buttons' labels and the funds in the
    /// place the HUD's clock has.
    fn pause_texts(&self, f: &Feed, out: &mut Vec<Text>) {
        f.hud.funds_text(f.view, f.measure, out);
        let h = crate::hud::TEXT_HEIGHT * PAUSE_SIZE_RATIO;
        let hh = Some(h * SCREEN.1);
        let tw = pause_width(f.measure, PAUSE_TITLE).0;
        out.push(Text { text: PAUSE_TITLE.into(), x: 0.5 - tw / SCREEN.0 / 2.0, y: PAUSE_TITLE_Y, shadow: false, width: Some(tw), height: hh, plain: true });
        for (label, x, y) in [PAUSE_ACCEPT, PAUSE_DECLINE] {
            let w = pause_width(f.measure, label).0;
            line(out, label, x + BUTTON_LABEL_SHIFT - w / SCREEN.0 / 2.0, y - h / 2.0, Some(w), hh);
        }
    }

    pub fn display_list(&self) -> Vec<super::display::DisplayItem> {
        self.player.display_list()
    }
}

/// A pause string's line width (pixels) and button width (fraction of the screen): the log's for
/// the strings it has, else the HUD's measure at the pause's size.
fn pause_width(measure: &Measure, text: &str) -> (f32, f32) {
    match PAUSE_WIDTHS.iter().find(|w| w.0 == text) {
        Some(&(_, px, button)) => (px, button),
        None => {
            let px = measure.hud_width(text) * PAUSE_SIZE_RATIO;
            (px, px / SCREEN.0)
        }
    }
}

/// The pause screen's title: the first section's (the section's own name comes with the
/// movie's section picker, which is not read).
const PAUSE_TITLE: &str = "GOALS";
