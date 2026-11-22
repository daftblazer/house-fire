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
use super::buy::Event;
use super::pause::{Game, Pause, Press, Request, Settings};
use crate::catalog::Purchase;
use super::player::{Command, Player};
use super::value::Value;
use crate::hud::{Call, Hud, Measure, PlayerView, SCREEN, Speed, Text};

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

/// The characters of the options movie's dialog body: the box's left and right edge and its
/// middle (`leftBox`, `middleBox`, `rightBox`).
const OPTIONS_BODY: [u32; 3] = [28, 31, 34];


/// The interaction menu as a list of pills: the rows' pitch (pixels), and how far a pill reaches
/// left of its label's centre beyond half the label (the A picture) and right of it (the pill's
/// end): read off the pause screen's SELECT pill (spans 183 to 305 pixels for a label 59.9 wide
/// centred on 256).
const MENU_PITCH: f32 = 44.0;
const PILL_LEFT: f32 = 43.0;
const PILL_RIGHT: f32 = 18.0;
/// A pill's half height in pixels (the pause screen's pills span 22 pixels each side of their y).
const PILL_HALF_HEIGHT: f32 = 22.0;
/// The picture's size, for keeping the menu inside it.
const MENU_MARGIN: f32 = 6.0;

/// A dialog's button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButton {
    Accept,
    Decline,
}

/// An interaction menu that is open: its entries, the highlighted one and the place of the first
/// row's label (pixels).
#[derive(Clone, Debug)]
struct Menu {
    entries: Vec<String>,
    selected: usize,
    at: (f32, f32),
    /// The pill is where `drawn` says (the entry highlighted, its place): drawn again when the
    /// highlight moves.
    drawn: Option<usize>,
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
    /// The pause screen's state while it is up.
    pause: Option<Pause>,
    /// The pause screen's strings and the save's funds, and the options it keeps between visits.
    game: Game,
    settings: Settings,
    /// The speed the HUD was last given outside the frame the pause screen opened in (the speed
    /// buttons follow the pause a frame later).
    speed_seen: Speed,
    speed_before: Speed,
    opened_at: Option<u32>,
    /// The pause screen closed in this frame (the HUD's speed is what it was before the pause).
    closing: bool,
    menu: Option<Menu>,
    /// The menu's pill is hidden again by the next frame's calls.
    menu_gone: bool,
    /// A call the pause screen's open or close waits to make.
    calls: Vec<Call>,
    /// Every command the movies sent, for a check against the log's.
    pub sent: Vec<Command>,
    /// Keeps what the game side did, for a check against the log (`take_record`).
    pub record: bool,
    /// The calls made into the movies and the movies loaded since the last `take_record`.
    made: Vec<(String, Vec<String>)>,
    loaded: Vec<String>,
    /// The pad's buttons held at the last `pad`.
    held: u16,
    /// Where the game's cursor is on the lot, in tiles (where place mode starts).
    cursor: (f32, f32),
    /// The price of the object being placed, held back from the funds shown until the world's
    /// funds have changed from what they were when it was taken (`base`): (price, base).
    reserve: Option<(i32, i32)>,
    /// What the household has spent on purchases the world has taken (its worth counts them).
    spent: i32,
    /// The real funds of the frame being run.
    funds_seen: i32,
    /// The objects placed and not yet taken by the owner.
    purchases: Vec<Purchase>,
    /// The dialogs' requests not yet taken.
    requests: Vec<Request>,
    /// A call whose movie only acts on it a frame later (`showInfoDialog`: the info window's
    /// motive lozenges start a frame after the game's call, 2027 and 2029), made at the end of the
    /// next frame.
    deferred: Vec<Call>,
}

/// The pad's buttons as the game reads them (PADStatus).
pub mod pad {
    pub const LEFT: u16 = 0x0001;
    pub const RIGHT: u16 = 0x0002;
    pub const DOWN: u16 = 0x0004;
    pub const UP: u16 = 0x0008;
    pub const A: u16 = 0x0100;
    pub const B: u16 = 0x0200;
    pub const START: u16 = 0x1000;
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
        Ui { player, frame: 0, first: true, announce: false, pause: None, game: Game::default(), settings: Settings::default(), speed_seen: Speed::Normal, speed_before: Speed::Normal, opened_at: None, closing: false, menu: None, menu_gone: false, calls: Vec::new(), sent: Vec::new(), record: false, made: Vec::new(), loaded: Vec::new(), held: 0, cursor: (0.0, 0.0), reserve: None, spent: 0, funds_seen: 0, purchases: Vec::new(), requests: Vec::new(), deferred: Vec::new() }
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
        self.pause.is_some()
    }

    /// The pause screen's strings and the save's funds (from the disc and the save).
    pub fn set_game(&mut self, game: Game) {
        self.game = game;
    }

    /// The game's options as the options screen keeps them.
    pub fn settings(&self) -> Settings {
        self.pause.as_ref().map_or(self.settings, Pause::settings)
    }

    /// A key goes down and up (the pad or keyboard, as the game passes it to the movies).
    pub fn press(&mut self, code: u32) {
        self.player.key(code, true);
        self.player.key(code, false);
    }

    /// The interaction menu opens at a place of the picture (pixels): `entries` as rows of pills, the
    /// highlighted one drawn with the A button. The game's own pie menu is drawn by the game
    /// itself and is not in the movies (`docs/assets.md`, "In the sandbox (U4)"): the pills are
    /// the wrapper's button, as the pause screen's, a stand-in for the original's.
    pub fn show_menu(&mut self, entries: Vec<String>, selected: usize, at: (f32, f32)) {
        self.menu = Some(Menu { selected: selected.min(entries.len().saturating_sub(1)), entries, at, drawn: None });
        self.menu_gone = false;
    }

    /// The menu's highlight moves to an entry.
    pub fn menu_select(&mut self, selected: usize) {
        if let Some(m) = &mut self.menu {
            m.selected = selected.min(m.entries.len().saturating_sub(1));
        }
    }

    /// The menu closes.
    pub fn hide_menu(&mut self) {
        if self.menu.take().is_some() {
            self.menu_gone = true;
        }
    }

    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The pixels of a menu row: the label's centre and width, the row's rectangle (left, top, right,
    /// bottom).
    fn row(m: &Menu, i: usize, measure: &Measure) -> ((f32, f32), f32, [f32; 4]) {
        let w = measure.plain_width(&m.entries[i]);
        let rows = m.entries.len() as f32;
        let (half_w, tall) = (w / 2.0, (rows - 1.0) * MENU_PITCH);
        // kept on the screen; a row or a menu wider or taller than it starts at its left or top
        let x = m.at.0.min(SCREEN.0 - MENU_MARGIN - PILL_RIGHT - half_w).max(MENU_MARGIN + PILL_LEFT + half_w);
        let y0 = m.at.1.min(SCREEN.1 - MENU_MARGIN - PILL_HALF_HEIGHT - tall).max(MENU_MARGIN + PILL_HALF_HEIGHT);
        let y = y0 + i as f32 * MENU_PITCH;
        ((x, y), w, [x - half_w - PILL_LEFT, y - PILL_HALF_HEIGHT, x + half_w + PILL_RIGHT, y + PILL_HALF_HEIGHT])
    }

    /// The menu entry under a point of the picture (pixels).
    pub fn menu_hit(&self, measure: &Measure, x: f32, y: f32) -> Option<usize> {
        let m = self.menu.as_ref()?;
        (0..m.entries.len()).find(|&i| {
            let r = Self::row(m, i, measure).2;
            x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
        })
    }

    /// The dialog's button under a point of the picture (pixels), by the places its movie reported
    /// for them (the pill spans from the A picture, left of the place, to its end).
    pub fn dialog_hit(&self, hud: &Hud, x: f32, y: f32) -> Option<DialogButton> {
        let l = hud.layout();
        [(DialogButton::Accept, l.accept), (DialogButton::Decline, l.decline)].into_iter().find_map(|(b, (bx, by))| {
            let (px, py) = (bx * SCREEN.0, by * SCREEN.1);
            (bx > 0.0 && x >= px - PILL_LEFT - 20.0 && x < px + PILL_RIGHT + 40.0 && (y - py).abs() < PILL_HALF_HEIGHT + 4.0).then_some(b)
        })
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
        self.pause = Some(Pause::new(self.settings));
        self.opened_at = Some(self.frame);
        self.speed_before = self.speed_seen;
    }

    /// The pause screen closes (B): its buttons destroyed, the HUD back (0x800A77F0 to 0x800A7960).
    pub fn close_pause(&mut self, hud: &mut Hud, view: &PlayerView) {
        if !self.paused() {
            return;
        }
        let calls = Self::close_calls();
        self.calls.extend(calls);
        self.leave(hud, view);
    }

    /// The calls that take the pause screen away.
    fn close_calls() -> Vec<Call> {
        let mut calls = vec![Call { name: "hidePause", args: Vec::new() }, Call { name: "setButtonContext", args: vec!["pause".into()] }];
        calls.extend(PAUSE_BUTTONS.map(|b| Call { name: "destroyButton", args: vec![b.into()] }));
        calls.push(Call { name: "resetButtonContext", args: Vec::new() });
        calls
    }

    /// The pause screen is gone: the HUD is back, the options are kept.
    fn leave(&mut self, hud: &mut Hud, view: &PlayerView) {
        hud.resume(view);
        self.closing = true;
        if let Some(mut p) = self.pause.take() {
            self.requests.extend(p.take_requests());
            self.settings = p.settings();
        }
    }

    /// Where the game's cursor is on the lot, in tiles: place mode starts there.
    pub fn set_cursor(&mut self, tile: (f32, f32)) {
        self.cursor = tile;
    }

    /// What the player chose in the pause screen's dialogs since the last call (the save, the
    /// quit), for the game's side to do.
    pub fn take_requests(&mut self) -> Vec<Request> {
        let mut out = std::mem::take(&mut self.requests);
        if let Some(p) = &mut self.pause {
            out.extend(p.take_requests());
        }
        out
    }

    /// The purchases the player has placed since the last call, for the simulation to apply
    /// (`Alone::buy`): the screen itself changes nothing.
    pub fn take_purchases(&mut self) -> Vec<Purchase> {
        std::mem::take(&mut self.purchases)
    }

    /// The pad as the game reads it this frame (the buttons held): Start opens the pause screen,
    /// and while it is open the arrows, A and B reach the movie that has the focus two frames
    /// later (`Pause::pad`).
    pub fn pad(&mut self, buttons: u16, hud: &mut Hud) {
        let pressed = buttons & !self.held;
        self.held = buttons;
        if pressed & pad::START != 0 && !self.paused() {
            self.open_pause(hud);
            return;
        }
        let frame = self.frame;
        if let Some(p) = &mut self.pause {
            p.pad(pressed, frame);
        }
    }

    /// The calls made into the movies (name, arguments) and the movies loaded since the last
    /// take, while `record` is on.
    pub fn take_record(&mut self) -> (Vec<(String, Vec<String>)>, Vec<String>) {
        (std::mem::take(&mut self.made), std::mem::take(&mut self.loaded))
    }

    /// The answers the game gives the movies' reads this frame.
    pub fn answers(&self, f: &Feed) -> Vec<(String, String)> {
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
        if let Some(p) = &self.pause {
            out.extend(p.answers(f.view, &self.game, f.measure.font));
        }
        out
    }

    /// Loads the movies that asked for it (the game loads what a movie's `loadMovie` names).
    fn load_pending(&mut self) {
        for name in self.player.pending_loads() {
            if self.record {
                self.loaded.push(name.clone());
            }
            self.player.load(&name);
        }
    }

    fn make(&mut self, c: &Call) {
        let a: Vec<Value> = c.args.iter().map(|s| Value::str(s)).collect();
        if self.player.trace {
            eprintln!("ui: frame {} call {}({:?})", self.frame, c.name, c.args);
        }
        if self.record {
            self.made.push((c.name.to_string(), c.args.clone()));
        }
        if c.name == "showInfoDialog" {
            self.deferred.push(c.clone());
            return;
        }
        self.dispatch(c.name, &a);
    }

    fn dispatch(&mut self, name: &'static str, a: &[Value]) {
        if !self.player.call(name, a) {
            self.player.note_stub(format!("call of {name}: no such function"));
        }
    }

    /// The commands the movies sent, as the game's handlers take them one at a time: the pause
    /// screen's first, which makes its calls at once, and the commands those bring are heard
    /// before the HUD's turn with this one. A section's `draw_X` "false" is heard after the
    /// calls the game makes around it (the log has it last: 1884, 1971, 2120). `late` is a drain
    /// after the movies' frame: the game's calls then count for one movie frame less before a
    /// button movie they ask for starts (1973 asks, 1976 reports).
    fn drain(&mut self, f: &mut Feed, late: bool) {
        let mut pending: std::collections::VecDeque<Command> = self.player.take_commands().into();
        let mut held: Vec<Command> = Vec::new();
        while let Some(c) = pending.pop_front() {
            if c.name.starts_with("draw_") && c.argument == "false" {
                held.push(c);
            } else {
                self.hear(c, f, late);
            }
            let mut fresh: std::collections::VecDeque<Command> = self.player.take_commands().into();
            fresh.append(&mut pending);
            pending = fresh;
        }
        for c in held {
            self.hear(c, f, late);
        }
    }

    fn hear(&mut self, c: Command, f: &mut Feed, late: bool) {
        let calls = match &mut self.pause {
            Some(p) => p.command(&c.name, &c.argument, self.frame, &self.game.text, f.measure.font),
            None => Vec::new(),
        };
        if late {
            self.player.cache_ticks -= 1;
        }
        for call in &calls {
            self.make(call);
        }
        if late {
            self.player.cache_ticks += 1;
        }
        let fresh = self.player.take_commands();
        for fc in fresh {
            self.hear(fc, f, late);
        }
        if c.name == "pauseDone" && c.argument == "true" && self.pause.is_some() {
            // the main row's movie says the player is done: the game takes the pause screen away
            let calls = Self::close_calls();
            for call in &calls {
                self.make(call);
            }
            self.leave(f.hud, f.view);
            let back = f.hud.update(f.view, self.speed_before, false);
            for call in &back {
                self.make(call);
            }
            let fresh = self.player.take_commands();
            for fc in fresh {
                self.hear(fc, f, late);
            }
        }
        f.hud.command(&c.name, &c.argument);
        self.sent.push(c);
    }

    /// The presses of the pad that are due: a key to the movie that has the focus, the pause
    /// screen closing, the options screen leaving.
    fn presses(&mut self, f: &mut Feed) {
        let Some(due) = self.pause.as_mut().map(|p| p.due(self.frame)) else { return };
        for press in due {
            match press {
                Press::Key(k) => {
                    self.press(k);
                    self.drain(f, false);
                }
                Press::Back => {
                    if self.pause.as_ref().is_some_and(Pause::dialog) {
                        self.press(keys::TRIANGLE);
                        self.drain(f, false);
                    }
                    let (calls, late) = self.pause.as_mut().map(|p| (p.back(), p.late_back())).unwrap_or_default();
                    if late {
                        // (the buy and build screens' calls come after the frame's loads)
                        self.calls.extend(calls);
                    } else {
                        for c in &calls {
                            self.make(c);
                        }
                        self.drain(f, false);
                    }
                }
                Press::ShopNow(mv) => {
                    let calls = self.pause.as_mut().map(|p| p.shop_now(mv)).unwrap_or_default();
                    for c in &calls {
                        self.make(c);
                    }
                    self.drain(f, false);
                }
                Press::Shop(mv) => {
                    let cursor = self.cursor;
                    if let Some(p) = &mut self.pause {
                        // (the screens' calls come after the frame's loads)
                        let calls = p.shop_apply(mv, self.frame, &self.game, f.measure.font, cursor);
                        self.calls.extend(calls);
                        for e in p.shop_events() {
                            match e {
                                Event::Reserve(price) => self.reserve = Some((price, self.funds_seen)),
                                Event::Release => self.reserve = None,
                                Event::Purchase(p) => self.purchases.push(p),
                            }
                        }
                    }
                }
                Press::OptionsLeave { keep } => {
                    if let Some(p) = &mut self.pause {
                        // (the options screen's calls come after the frame's loads)
                        let calls = p.leave_options(keep);
                        self.calls.extend(calls);
                    }
                }
            }
        }
        // (what the open screen's list makes every frame, after the change of screen of the frame)
        let frame_calls = self.pause.as_ref().map(|p| p.shop_frame_calls(&self.game, f.measure.font, self.frame)).unwrap_or_default();
        self.calls.extend(frame_calls);
    }

    /// One presentation frame: the movies run a frame, load what they asked for, the game hears
    /// what they sent and makes its calls; the lines of text it draws are given back.
    pub fn step(&mut self, outer: &mut Feed) -> Vec<Text> {
        // the funds as the screen shows them: the price of the object being placed is already taken
        let mut view = outer.view.clone();
        self.funds_seen = view.funds;
        if let Some((price, base)) = self.reserve {
            if view.funds == base {
                view.funds -= price;
            } else {
                self.spent += price;
                self.reserve = None;
            }
        }
        let mut inner = Feed { hud: &mut *outer.hud, view: &view, speed: outer.speed, measure: outer.measure };
        let f = &mut inner;
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
        // the speed buttons follow the pause screen's opening a frame later
        let frame = self.frame;
        let mut speed = if self.opened_at.take_if(|at| *at == frame).is_some() { self.speed_seen } else { f.speed };
        self.speed_seen = speed;
        let answers = self.answers(f);
        self.player.set_answers(answers);
        self.presses(f);
        self.player.advance();
        self.drain(f, false);
        self.player.tick_loads();
        self.drain(f, true);
        self.load_pending();
        self.drain(f, false);
        if std::mem::take(&mut self.closing) {
            speed = self.speed_before;
        }
        let mut calls = std::mem::take(&mut self.calls);
        calls.extend(f.hud.update(f.view, speed, std::mem::take(&mut self.announce)));
        let (draw, mut texts) = f.hud.draw(f.view, f.measure);
        calls.extend(draw);
        self.menu_calls(f, &mut calls);
        let earlier = std::mem::take(&mut self.deferred);
        for c in &calls {
            self.make(c);
        }
        for c in &earlier {
            let a: Vec<Value> = c.args.iter().map(|s| Value::str(s)).collect();
            self.dispatch(c.name, &a);
        }
        self.drain(f, false);
        self.menu_texts(f, &mut texts);
        if let Some(p) = &self.pause {
            p.texts(f.view, &self.game, f.measure.font, &f.hud.layout(), self.frame, &mut texts);
            if p.funds_shown() {
                // (the price of an object whose place mode has just begun is already taken)
                let shown = match self.reserve {
                    Some((price, base)) if self.funds_seen == base => base - price,
                    _ => self.funds_seen,
                };
                f.hud.funds_line(shown, f.hud.layout().money, f.measure, &mut texts);
                let worth = self.game.family_funds + self.spent + self.reserve.map_or(0, |r| r.0);
                f.hud.funds_line(worth, f.hud.layout().time, f.measure, &mut texts);
            }
        }
        self.frame += 1;
        texts
    }

    /// The calls that put the menu's pill on the highlighted row, and take it away.
    fn menu_calls(&mut self, f: &Feed, calls: &mut Vec<Call>) {
        if let Some(m) = self.menu.as_mut().filter(|m| m.drawn != Some(m.selected)) {
            let ((x, y), w, _) = Self::row(m, m.selected, f.measure);
            calls.push(Call { name: "setButtonContext", args: vec!["menu".into()] });
            calls.push(Call { name: "drawButton", args: vec!["accept".into(), format!("{:.6}", x / SCREEN.0), format!("{:.6}", y / SCREEN.1), format!("{:.6}", w / SCREEN.0)] });
            calls.push(Call { name: "resetButtonContext", args: Vec::new() });
            m.drawn = Some(m.selected);
        }
        if std::mem::take(&mut self.menu_gone) {
            calls.push(Call { name: "setButtonContext", args: vec!["menu".into()] });
            calls.push(Call { name: "drawButton", args: vec!["accept".into(), "-1".into(), "-1".into(), ".1".into()] });
            calls.push(Call { name: "resetButtonContext", args: Vec::new() });
        }
    }

    /// The menu's labels: plain draws (as the game's "Go Here" is), left of nothing in particular,
    /// centred on the pill's label place.
    fn menu_texts(&self, f: &Feed, out: &mut Vec<Text>) {
        let Some(m) = &self.menu else { return };
        for (i, e) in m.entries.iter().enumerate() {
            let ((x, y), w, _) = Self::row(m, i, f.measure);
            // the glyphs are `TEXT_HEIGHT` of the screen tall, the label's top is half of it above its middle
            out.push(Text { text: e.clone(), x: (x - w / 2.0) / SCREEN.0, y: y / SCREEN.1 - crate::hud::TEXT_HEIGHT / 2.0, shadow: false, width: None, height: None, plain: true });
        }
    }

    /// What the movies show, back to front. On the options screen the dialog's body (the three
    /// pieces of its box) is in the picture twice over: the game's panel is 0.96 opaque over the
    /// world where the shapes give 0.81 once (measured on frame 2500 of pause-c.log at two
    /// places with different pictures under them: 0.81 and 0.19 make 0.96 when stacked). What
    /// puts the second copy there is not known.
    pub fn display_list(&self) -> Vec<super::display::DisplayItem> {
        let list = self.player.display_list();
        if !self.pause.as_ref().is_some_and(Pause::on_options) {
            return list;
        }
        let mut out = Vec::with_capacity(list.len() + 3);
        for i in list {
            let twice = &*i.movie == "options" && OPTIONS_BODY.contains(&i.character);
            if twice {
                out.push(i.clone());
            }
            out.push(i);
        }
        out
    }
}
