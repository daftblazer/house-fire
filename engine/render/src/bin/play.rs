//! The sandbox: a window in which the player plays the lot, the engine's own simulation running
//! with no log after its start (`housefire::replay::Alone`), drawn each frame as `view` draws it,
//! with the game's own interface over it: the HUD, the dialog and the pause screen are the Apt
//! movies run by the engine's feed (`housefire::apt::ui`), the interaction menu its pills.
//!
//! Usage: play LOG ISO --card CARD [--lot N --hood CARD] [--seed N] [--script FILE]
//!             [--headless] [--ticks N] [--shot TICK FILE] [--mute] [--wav FILE] [--device]
//!
//! The log is read for the start only (as `replay --alone`) and for the first camera. Controls:
//!   Space pause (the pause screen: arrows and Enter move in it, Space or Esc closes it); 1, 2, 3 speed (ticks a frame); Tab next Sim of the household; F camera to the Sim
//!   Left click: a Sim of the household selects it; another object opens its menu for the active
//!   Sim; the floor makes a "go here" marker. Click an entry (or Up/Down and Enter), Esc closes.
//!   Camera: Q/E or the right stick turn, wheel / +/- / triggers zoom, WASD / arrows / middle
//!   drag / left stick pan. Gamepad: Start pauses, shoulders change the speed, A clicks at the
//!   middle of the picture (or chooses), B closes, Y changes the Sim, D-pad moves in a menu.
//!
//! --script: lines of `TICK command`, applied as the input would be at that tick (`pick ID`, `zoom DISTANCE`,
//! `floor X Y`, `focus`, `menu`, `choose N` or `choose @ACTION`, `answer N`, `pause` (the pause screen opens or closes), `key CODE` (a key to the dialog's or pause screen's movie: 37 to 40 the arrows, 88 A, 79 B), `speed K`, `sim ID`,
//! `expect SIM TREE WITHIN`, `quit`); with --headless no window is made (and no GPU unless --shot
//! is given). --shot: the picture of a tick (with the interface) saved as PNG.
//! Each command queued is printed: `play: tick T sim S target O action A (name)`.

use ash::vk;
use housefire::apt::ui::{DialogButton, Feed, Ui, keys};
use housefire::disc::Disc;
use housefire::hud::{Dialog, Hud, Measure, PlayerView, Speed};
use housefire::library::Library;
use housefire::replay::{Alone, DialogAsk, MenuEntry, RunWith, Start, atoi};
use housefire::scene::Camera;
use housefire::tree_log::{Log, read_log};
use housefire::world::{Object, World};
use housefire_render::audio::Sound;
use housefire_render::engine_scene::{self, Builder, Lot};
use housefire_render::frame::{Frame, Mat3x4, TRANSIENT_MESH};
use housefire_render::gpu::Gpu;
use housefire_render::apt_draw::{AptAssets, Painter};
use housefire_render::game_text;
use housefire_render::pick::{self, Pick};
use housefire_render::renderer::{Present, Renderer};
use housefire_render::scene::Assets;
use housefire_render::sims::{self, SimSkins, Wardrobe};
use housefire_render::window::{self, App, Swapchain};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// The picture's size: the game's own (the reference pictures'); the window shows it doubled.
const WIDTH: u32 = 640;
const HEIGHT: u32 = 448;
const WINDOW_SCALE: u32 = 2;
/// The pictures a second: the original's frames (`routing::FRAME_MS`), a tick every one or two
/// of them (`routing::TICK_MS`), so a walk shows a step a picture.
const FRAMES_PER_SECOND: f64 = 30.0;
/// How long before a frame's time the wait turns from sleeping into spinning.
const SPIN: Duration = Duration::from_micros(1500);
/// The farthest the zoom goes: the farthest distance in the home log's camera records (their
/// distances run 8 to 17.67, the club's 8 to 14, and the tilt 25 to 36.1 with them, as
/// `Camera::zoom` gives it). The original's code allows up to `Camera::FAR` (35), which no log has
/// drawn, so it is not offered.
const ZOOM_FAR: f32 = 17.666_666;
/// What a wheel notch, and a second of the zoom keys, move the zoom (tiles; chosen).
const WHEEL_ZOOM: f32 = 1.5;
const KEY_ZOOM_RATE: f32 = 10.0;
/// Stick values under this are taken as rest (chosen).
const DEAD_ZONE: f32 = 0.2;
/// The tick limit of a headless run without `quit`: this many past the script's last line.
const SCRIPT_TAIL: u32 = 600;
/// The home lot's house number when the save names none (as `view`'s).
const HOME_HOUSE: u32 = 1;
/// The presentation frame the HUD is shown at (`showHUD`): the sandbox has no load screen to wait
/// out, so the wrapper has its few frames to start and `f_hud` to load (the game's load bar takes
/// the 39 frames between `loadFunctions` and `showHUD` in the ui-dump log).
const HUD_FRAME: u32 = 6;
/// The font the interface's text is drawn in (`FRANKFUR`, the CRC-32 of its name).
const INTERFACE_FONT: u32 = 0x67f6_64fb;
/// The household's Sims: person data 32 (the visitors' mark) is 0.
const PERSON_VISITOR: usize = 32;
/// The interface's sounds (the bank's `UI_accept` and `UI_move`): a menu opening and choosing
/// accept, the highlight moving moves.
const UI_OPEN: &str = "ui_accept";
const UI_CHOOSE: &str = "ui_accept";
const UI_MOVE: &str = "ui_move";
/// The music of the start: the load's loop plays for this many ticks (5 s; chosen: the sandbox has
/// no load screen to end it).
const LOAD_MUSIC: &str = "loadloop";
const LOAD_MUSIC_TICKS: u32 = 150;
/// Mixed into the run's seed for the presentation's own random numbers (the sounds' variants).
const SOUND_SEED: u64 = 0x5055_4E44;

struct Args {
    log: String,
    iso: String,
    card: Option<String>,
    hood: Option<String>,
    lot: i64,
    seed: u64,
    script: Option<PathBuf>,
    headless: bool,
    ticks: Option<u32>,
    shot: Option<(u32, PathBuf)>,
    /// No sound.
    mute: bool,
    /// A headless run's sound, mixed and written as a WAV (a window plays through the device).
    wav: Option<PathBuf>,
    /// A headless run plays through the device too (it runs faster than real time, so this only
    /// checks that a device opens).
    device: bool,
}

fn args() -> Option<Args> {
    let a: Vec<String> = std::env::args().collect();
    let mut out = Args { log: a.get(1)?.clone(), iso: a.get(2)?.clone(), card: None, hood: None, lot: -1, seed: 1, script: None, headless: false, ticks: None, shot: None, mute: false, wav: None, device: false };
    let mut i = 3;
    while i < a.len() {
        match a[i].as_str() {
            "--card" => out.card = Some(a.get(i + 1)?.clone()),
            "--hood" => out.hood = Some(a.get(i + 1)?.clone()),
            "--lot" => out.lot = atoi(a.get(i + 1)?),
            "--seed" => out.seed = atoi(a.get(i + 1)?) as u64,
            "--script" => out.script = Some(a.get(i + 1)?.into()),
            "--ticks" => out.ticks = Some(a.get(i + 1)?.parse().ok()?),
            "--headless" | "--mute" | "--device" => {
                match a[i].as_str() {
                    "--mute" => out.mute = true,
                    "--device" => out.device = true,
                    _ => out.headless = true,
                }
                i += 1;
                continue;
            }
            "--wav" => out.wav = Some(a.get(i + 1)?.into()),
            "--shot" => {
                out.shot = Some((a.get(i + 1)?.parse().ok()?, a.get(i + 2)?.into()));
                i += 1;
            }
            _ => return None,
        }
        i += 2;
    }
    Some(out)
}

/// What the player (or a script) does.
#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    /// A click on the picture at a point (the picture's pixels).
    Click(f32, f32),
    /// A click on an object, by id (as if on its place).
    Pick(i16),
    /// A click on the floor at a point of the lot (tiles).
    Floor(f32, f32),
    /// Open the menu of the last target again.
    Menu,
    Choose(usize),
    ChooseAction(u16),
    Answer(usize),
    Speed(u32),
    TogglePause,
    NextSim,
    SetSim(i16),
    FocusSim,
    /// The camera's zoom set to a distance (tiles; a script's, for the sound checks).
    Zoom(f32),
    Close,
    MenuMove(i32),
    /// Enter: the menu's or the dialog's highlighted entry.
    Confirm,
    /// A key code for the dialog's or the pause screen's movie (an arrow).
    Key(u32),
    Quit,
    /// A check of the run: the Sim's stack must enter the tree within so many ticks of now.
    Expect(i16, i32, u32),
}

/// A script's line: `TICK command args`.
fn parse_line(line: &str) -> Option<(u32, Cmd)> {
    let line = line.split('#').next()?.trim();
    let mut w = line.split_whitespace();
    let tick = w.next()?.parse().ok()?;
    let word = w.next()?;
    let n = |w: &mut std::str::SplitWhitespace| w.next().and_then(|s| s.parse::<i32>().ok());
    let f = |w: &mut std::str::SplitWhitespace| w.next().and_then(|s| s.parse::<f32>().ok());
    let cmd = match word {
        "pick" => Cmd::Pick(n(&mut w)? as i16),
        "floor" => Cmd::Floor(f(&mut w)?, f(&mut w)?),
        "menu" => Cmd::Menu,
        "pause" => Cmd::TogglePause,
        "key" => Cmd::Key(n(&mut w)? as u32),
        "focus" => Cmd::FocusSim,
        "zoom" => Cmd::Zoom(f(&mut w)?),
        "choose" => {
            let a = w.next()?;
            match a.strip_prefix('@') {
                Some(action) => Cmd::ChooseAction(action.parse().ok()?),
                None => Cmd::Choose(a.parse().ok()?),
            }
        }
        "answer" => Cmd::Answer(n(&mut w)? as usize),
        "speed" => Cmd::Speed(n(&mut w)? as u32),
        "sim" => Cmd::SetSim(n(&mut w)? as i16),
        "expect" => Cmd::Expect(n(&mut w)? as i16, n(&mut w)?, n(&mut w)? as u32),
        "quit" => Cmd::Quit,
        _ => return None,
    };
    Some((tick, cmd))
}

/// A menu that is open.
struct MenuState {
    target: i16,
    items: Vec<MenuEntry>,
    selected: usize,
}

/// An `expect` of a script: the Sim's stack must hold the tree by the deadline.
struct Expect {
    sim: i16,
    tree: i32,
    asked: u32,
    deadline: u32,
    seen: Option<u32>,
}

/// The player's state around the simulation: the active Sim, the menu, the speed, the camera.
struct Game<'l> {
    alone: Alone<'l>,
    /// What the Sims did in the ticks whose frames are not all shown yet, for their figure
    /// animators, and how many of the first's frames are shown.
    sim_frames: std::collections::VecDeque<(u32, Vec<housefire::replay::SimFrame>)>,
    first_shown: u32,
    /// The frames of the ticks run not yet shown.
    frames_ready: u32,
    animators: sims::Animators,
    /// Whether the pictures take the Sims' figures (else `sim_frames` is not kept).
    figures: bool,
    lib: &'l Library,
    camera: Camera,
    active: Option<i16>,
    target: Option<i16>,
    menu: Option<MenuState>,
    dialog: Option<(DialogAsk, String, Vec<String>, usize)>,
    /// The made Sims' names by neighbour id (the save's).
    names: std::collections::HashMap<i16, String>,
    /// Ticks a frame: 0 paused.
    speed: u32,
    last_speed: u32,
    quit: bool,
    expects: Vec<Expect>,
    /// The sound, if there is any (`--mute`, no device and a headless run without `--wav` have none).
    sound: Option<Sound>,
    /// The interface: the movies, what the game feeds them, the font its text is measured in, and
    /// the lines it drew in the last frame.
    ui: Ui,
    hud: Hud,
    font: housefire::apt::font::Font,
    texts: Vec<housefire::hud::Text>,
    /// The presentation frames run (the interface's clock).
    frames: u32,
    /// The frames run with the simulation standing still (the pause screen up): a script's and
    /// a shot's ticks count them too, so a script can press keys in the pause screen.
    paused_frames: u32,
}

impl<'l> Game<'l> {
    /// An interface sound (the menu's open, move and choose).
    fn ui(&mut self, name: &str) {
        let tick = self.alone.ticks();
        if let Some(s) = &mut self.sound {
            s.ui(tick, name);
        }
    }

    fn household(&self) -> Vec<i16> {
        let mut v: Vec<i16> = self.alone.world().objects().filter(|o| o.person && o.on_lot() && o.person_data.get(PERSON_VISITOR) == Some(&0)).map(|o| o.id()).collect();
        v.sort_unstable();
        v
    }

    /// A Sim's own name: the save's made Sim of its neighbour record, else its record's name.
    fn sim_name(&self, id: i16) -> String {
        let w = self.alone.world();
        let neighbour = w.find_object(id).and_then(|o| o.person_data.get(housefire::world::PERSON_NEIGHBOUR_ID).copied());
        neighbour
            .and_then(|n| self.names.get(&n).cloned().or_else(|| w.neighbours.iter().find(|x| x.id == n).map(|x| x.name.clone())))
            .unwrap_or_else(|| format!("Sim {id}"))
    }

    fn object_name(&self, id: i16) -> String {
        let w = self.alone.world();
        match w.find_object(id) {
            Some(o) if o.person => self.sim_name(id),
            // its catalog name (the file's CTSS), else its file's name
            Some(o) => o.file.map_or_else(
                || format!("object {id}"),
                |f| {
                    let file = self.lib.file(f);
                    file.catalog.values().next().and_then(|s| s.first()).filter(|t| !t.is_empty()).cloned()
                        .unwrap_or_else(|| file.path.rsplit('/').next().unwrap_or("").trim_end_matches(".iff").to_string())
                },
            ),
            None => format!("object {id}"),
        }
    }

    fn view(&self) -> (Mat3x4, [f32; 6]) {
        (self.camera.view(), Camera::projection(self.camera.distance))
    }

    /// Whether a click on this object means something: a Sim, or an object with a menu.
    fn clickable(world: &World, lib: &Library, o: &Object) -> bool {
        o.person || lib.table_of(o.file, o.person, world.definition(o)).table.is_some()
    }

    fn next_sim(&mut self) {
        let h = self.household();
        if h.is_empty() {
            return;
        }
        let at = self.active.and_then(|a| h.iter().position(|&s| s == a));
        self.active = Some(h[at.map_or(0, |i| (i + 1) % h.len())]);
    }

    /// The point on the picture a place of the lot is at (the middle if it cannot be told).
    fn screen_of(&self, id: i16) -> (i32, i32) {
        let (view, proj) = self.view();
        self.alone.world().find_object(id).and_then(|o| pick::project(&view, &proj, [o.x16 as f32 / 16.0, o.y16 as f32 / 16.0, pick::OBJECT_HEIGHT], (WIDTH as f32, HEIGHT as f32))).map_or((WIDTH as i32 / 2, HEIGHT as i32 / 2), |(x, y)| (x as i32, y as i32))
    }

    fn open_menu(&mut self, target: i16, at: (i32, i32)) {
        let Some(sim) = self.active else { return };
        let items = self.alone.menu(sim, target);
        let listed: Vec<String> = items.iter().map(|e| format!("{} {}", e.action, e.name)).collect();
        println!("play: tick {} menu of object {target} for sim {sim}: [{}]", self.alone.ticks(), listed.join(", "));
        self.target = Some(target);
        let names: Vec<String> = self.menu_names(&items);
        self.ui.show_menu(names, 0, (at.0 as f32, at.1 as f32));
        self.menu = Some(MenuState { target, items, selected: 0 });
        self.ui(UI_OPEN);
    }

    /// The entries' names as the menu shows them.
    fn menu_names(&self, items: &[MenuEntry]) -> Vec<String> {
        if items.is_empty() {
            return vec!["(nothing)".to_string()];
        }
        items.iter().map(|e| e.name.clone()).collect()
    }

    fn close_menu(&mut self) {
        self.menu = None;
        self.ui.hide_menu();
    }

    fn click_object(&mut self, id: i16, at: (i32, i32)) {
        if self.household().contains(&id) {
            self.active = Some(id);
            self.close_menu();
        } else {
            self.open_menu(id, at);
        }
    }

    /// A click on the floor: the "go here" marker made where it is and queued on the active Sim.
    fn go_here(&mut self, at: [f32; 2]) {
        let Some(sim) = self.active else { return };
        let lib = self.lib;
        let Some(guid) = lib.find("destination_gohereandshow").and_then(|f| lib.file(f).definitions.first().map(|d| d.guid())) else {
            println!("play: no go-here marker on the disc");
            return;
        };
        let level = self.alone.world().find_object(sim).map_or(1, |o| o.level);
        // the marker stands at the middle of the tile clicked (the log's is at 392, 344: tile 24, 21)
        let Some(marker) = self.alone.make_object(guid, (at[0].floor() as i32 * 16 + 8, at[1].floor() as i32 * 16 + 8), level) else { return };
        let o = self.alone.world().find_object(marker).expect("made");
        let tab = lib.table_of(o.file, false, self.alone.world().definition(o));
        let Some(action) = tab.file.zip(tab.table).and_then(|(f, t)| lib.table(f, t).first()).map(|e| e.action) else { return };
        self.queue(sim, marker, action, "go here");
    }

    fn queue(&mut self, sim: i16, target: i16, action: u16, name: &str) {
        let ok = self.alone.queue_command(sim, target, action);
        println!("play: tick {} sim {sim} target {target} action {action} ({name}){}", self.alone.ticks(), if ok { "" } else { " REFUSED" });
    }

    fn choose(&mut self, i: usize) {
        let Some(m) = self.menu.take() else { return };
        self.ui.hide_menu();
        let (Some(sim), Some(e)) = (self.active, m.items.get(i)) else { return };
        self.ui(UI_CHOOSE);
        self.queue(sim, m.target, e.action, &e.name.clone());
    }

    fn answer(&mut self, button: usize) {
        if self.dialog.take().is_some() {
            println!("play: tick {} dialog answered with button {button}", self.alone.ticks());
            self.alone.answer_dialog(button == 0);
            self.hud.hide_dialog();
        }
    }

    /// The pause screen opens or closes (Space, Start); the simulation stands still while it is up.
    fn toggle_pause(&mut self) {
        let Some(view) = self.view_of_player() else { return };
        if self.ui.paused() {
            self.ui.close_pause(&mut self.hud, &view);
            self.speed = self.last_speed.max(1);
        } else {
            self.close_menu();
            self.ui.open_pause(&mut self.hud);
            if self.speed > 0 {
                self.last_speed = self.speed;
            }
            self.speed = 0;
        }
    }

    /// The script's clock: the simulation's ticks and the frames the pause screen stood for.
    fn clock(&self) -> u32 {
        self.alone.ticks() + self.paused_frames
    }

    /// Whether the dialog or the pause screen has the keys.
    fn focus(&self) -> bool {
        self.dialog.is_some() || self.ui.paused()
    }

    /// What the HUD shows of the active Sim.
    fn view_of_player(&self) -> Option<PlayerView> {
        PlayerView::of(self.alone.world(), self.active?)
    }

    /// One frame of the interface: the movies run, the game feeds them, and a button the player
    /// pressed on the dialog is answered. Keeps the lines of text the game drew.
    fn frame_ui(&mut self) {
        self.frames += 1;
        let Some(view) = self.view_of_player() else { return };
        if self.frames == HUD_FRAME {
            self.ui.begin_play(&mut self.hud, &view);
        }
        let speed = match self.speed {
            0 => Speed::Paused,
            1 => Speed::Normal,
            2 => Speed::Fast,
            _ => Speed::Fastest,
        };
        let measure = Measure { font: &self.font };
        self.texts = self.ui.step(&mut Feed { hud: &mut self.hud, view: &view, speed, measure: &measure });
        if let Some(key) = self.hud.take_dialog_key() {
            self.answer(usize::from(key != "accept"));
        }
    }

    /// A click or key: the dialog takes it first, then an open menu, then the picture.
    fn apply(&mut self, cmd: Cmd) {
        if self.dialog.is_some() {
            match cmd {
                Cmd::Answer(b) => self.answer(b),
                // the keys as the pad passes them to the dialog's movie, which reports the button
                Cmd::Confirm => self.ui.press(keys::A),
                Cmd::Close => self.ui.press(keys::B),
                Cmd::Key(k) => self.ui.press(k),
                Cmd::Click(x, y) => match self.ui.dialog_hit(&self.hud, x, y) {
                    Some(DialogButton::Accept) => self.ui.press(keys::A),
                    Some(DialogButton::Decline) => self.ui.press(keys::B),
                    None => {}
                },
                Cmd::Quit => self.quit = true,
                Cmd::Speed(k) => self.speed = k.min(3),
                _ => {}
            }
            return;
        }
        if self.ui.paused() {
            // the pause screen has the keys: the pad's to its movie, B closes it
            match cmd {
                Cmd::Quit => self.quit = true,
                Cmd::Close | Cmd::TogglePause => self.toggle_pause(),
                Cmd::Confirm | Cmd::Click(..) => self.ui.press(keys::A),
                Cmd::Key(k) => self.ui.press(k),
                Cmd::Speed(k) if k > 0 => self.last_speed = k.min(3),
                _ => {}
            }
            return;
        }
        match cmd {
            Cmd::Quit => self.quit = true,
            Cmd::Speed(k) => {
                self.speed = k.min(3);
                if k > 0 {
                    self.last_speed = self.speed;
                }
            }
            Cmd::TogglePause => self.toggle_pause(),
            Cmd::NextSim => self.next_sim(),
            Cmd::SetSim(id) => self.active = Some(id),
            Cmd::FocusSim => {
                if let Some(o) = self.active.and_then(|a| self.alone.world().find_object(a)) {
                    self.camera.target = [o.x16 as f32 / 16.0, o.y16 as f32 / 16.0, self.camera.target[2]];
                }
            }
            Cmd::Zoom(d) => {
                let by = self.camera.distance - d;
                self.camera.zoom(by, ZOOM_FAR);
            }
            Cmd::Close => self.close_menu(),
            Cmd::MenuMove(d) => {
                if let Some(m) = &mut self.menu {
                    let n = m.items.len().max(1) as i32;
                    m.selected = (m.selected as i32 + d).rem_euclid(n) as usize;
                    let selected = m.selected;
                    self.ui.menu_select(selected);
                    self.ui(UI_MOVE);
                }
            }
            Cmd::Confirm => {
                if let Some(i) = self.menu.as_ref().map(|m| m.selected) {
                    self.choose(i);
                }
            }
            Cmd::Choose(i) => self.choose(i),
            Cmd::ChooseAction(a) => {
                if let Some(i) = self.menu.as_ref().and_then(|m| m.items.iter().position(|e| e.action == a)) {
                    self.choose(i);
                } else {
                    println!("play: tick {} no entry for action {a} in the menu", self.alone.ticks());
                }
            }
            Cmd::Answer(_) | Cmd::Key(_) => {}
            Cmd::Menu => {
                if let Some(t) = self.target {
                    let at = self.screen_of(t);
                    self.open_menu(t, at);
                }
            }
            Cmd::Pick(id) => {
                let at = self.screen_of(id);
                self.click_object(id, at);
            }
            Cmd::Floor(x, y) => self.go_here([x, y]),
            Cmd::Click(x, y) => self.click(x, y),
            Cmd::Expect(sim, tree, within) => {
                let now = self.alone.ticks();
                self.expects.push(Expect { sim, tree, asked: now, deadline: now + within, seen: None });
            }
        }
    }

    fn click(&mut self, x: f32, y: f32) {
        if self.menu.is_some() {
            let measure = Measure { font: &self.font };
            match self.ui.menu_hit(&measure, x, y) {
                Some(i) => self.choose(i),
                None => self.close_menu(),
            }
            return;
        }
        let (view, proj) = self.view();
        let world = self.alone.world();
        let lib = self.lib;
        let size = world.walls.size();
        let hit = pick::pick(world, &view, &proj, (x, y), (WIDTH as f32, HEIGHT as f32), (size.0 as f32, size.1 as f32), |o| Self::clickable(world, lib, o));
        match hit {
            Pick::Object(id) => self.click_object(id, (x as i32, y as i32)),
            Pick::Floor(p) => self.go_here(p),
            Pick::Nothing => {}
        }
    }

    /// The camera moved by a frame's input: `orbit` and `zoom` in -1..1 (a key or a stick),
    /// `zoom_by` a distance (the wheel), `pan` in the picture's axes (right, forward) -1..1.
    fn move_camera(&mut self, dt: f32, orbit: f32, zoom: f32, zoom_by: f32, pan: (f32, f32)) {
        let c = &mut self.camera;
        c.turn(orbit, dt);
        let step = zoom * KEY_ZOOM_RATE * dt + zoom_by;
        if step != 0.0 {
            c.zoom(step, ZOOM_FAR);
        }
        if pan != (0.0, 0.0) {
            let eye = c.eye();
            let (fx, fy) = (c.target[0] - eye[0], c.target[1] - eye[1]);
            let n = f32::hypot(fx, fy).max(1e-6);
            let (fx, fy) = (fx / n, fy / n);
            let speed = Camera::cursor_speed(c.distance) * dt;
            c.target[0] += (fx * pan.1 + fy * pan.0) * speed;
            c.target[1] += (fy * pan.1 - fx * pan.0) * speed;
            self.keep_on_lot();
        }
    }

    fn keep_on_lot(&mut self) {
        let size = self.alone.world().walls.size();
        for (k, s) in [size.0, size.1].into_iter().enumerate() {
            self.camera.target[k] = self.camera.target[k].clamp(1.0, (s as f32 - 1.0).max(1.0));
        }
    }

    /// One tick of the simulation (the dialog, if one opens, taken from the run).
    fn tick(&mut self) {
        self.alone.tick();
        self.frames_ready += self.alone.tick_frames();
        if self.figures {
            self.sim_frames.push_back((self.alone.tick_frames(), self.alone.sim_frames()));
        }
        let now = self.alone.ticks();
        if let Some(s) = &mut self.sound {
            let requests = self.alone.take_sound_requests();
            s.tick(now, &requests, &self.camera, self.alone.world());
            if now == LOAD_MUSIC_TICKS {
                s.music(None);
            }
        }
        if self.dialog.is_none()
            && let Some(ask) = self.alone.dialog()
        {
            let (text, buttons) = self.alone.dialog_text(&ask, &|id| self.object_name(id));
            println!("play: tick {} dialog from object {}: {text} {buttons:?}", self.alone.ticks(), ask.object);
            let world = self.alone.world();
            let strings: Vec<String> = world.find_object(ask.object).and_then(|o| o.file).and_then(|f| self.lib.file(f).strings.get(&301)).cloned().unwrap_or_default();
            let measure = Measure { font: &self.font };
            let shown = Dialog::opened(&strings, ask.operand, &ask.locals, &|id| self.object_name(id), &measure);
            self.hud.show_dialog(shown);
            self.dialog = Some((ask, text, buttons, 0));
        }
        let now = self.alone.ticks();
        for k in 0..self.expects.len() {
            let e = &self.expects[k];
            if e.seen.is_none() && self.alone.frames_of(e.sim).iter().skip(1).any(|f| f.0 == e.tree) {
                println!("play: tick {now} sim {} entered tree {} ({} ticks after the command)", e.sim, e.tree, now - e.asked);
                self.expects[k].seen = Some(now);
            }
        }
    }
}

/// The keys and the stick, as the frame sees them.
#[derive(Default)]
struct Input {
    held: HashSet<KeyCode>,
    /// The cursor in the window's pixels, and the last point of a middle drag (the picture's).
    cursor: (f32, f32),
    drag: Option<(f32, f32)>,
    wheel: f32,
    left_stick: (f32, f32),
    right_stick: (f32, f32),
    triggers: (f32, f32),
}

fn dead(v: f32) -> f32 {
    if v.abs() < DEAD_ZONE { 0.0 } else { v }
}

impl Input {
    fn key(&self, k: KeyCode) -> f32 {
        f32::from(u8::from(self.held.contains(&k)))
    }

    /// The window's events as commands, and the state the frame's camera moves read.
    fn events(&mut self, events: Vec<WindowEvent>, window: PhysicalSize<u32>, game: &mut Game) -> Vec<Cmd> {
        let to_picture = |p: (f32, f32)| (p.0 / window.width.max(1) as f32 * WIDTH as f32, p.1 / window.height.max(1) as f32 * HEIGHT as f32);
        let mut cmds = Vec::new();
        for e in events {
            match e {
                WindowEvent::KeyboardInput { event, .. } => {
                    let PhysicalKey::Code(code) = event.physical_key else { continue };
                    if event.state == ElementState::Released {
                        self.held.remove(&code);
                        continue;
                    }
                    self.held.insert(code);
                    if event.repeat {
                        continue;
                    }
                    let menu = game.menu.is_some();
                    let focus = game.focus();
                    match code {
                        KeyCode::Space => cmds.push(Cmd::TogglePause),
                        KeyCode::Digit1 | KeyCode::Numpad1 => cmds.push(Cmd::Speed(1)),
                        KeyCode::Digit2 | KeyCode::Numpad2 => cmds.push(Cmd::Speed(2)),
                        KeyCode::Digit3 | KeyCode::Numpad3 => cmds.push(Cmd::Speed(3)),
                        KeyCode::Tab => cmds.push(Cmd::NextSim),
                        KeyCode::KeyF => cmds.push(Cmd::FocusSim),
                        KeyCode::Escape => cmds.push(Cmd::Close),
                        KeyCode::Enter | KeyCode::NumpadEnter => cmds.push(Cmd::Confirm),
                        KeyCode::ArrowUp if focus => cmds.push(Cmd::Key(keys::UP)),
                        KeyCode::ArrowDown if focus => cmds.push(Cmd::Key(keys::DOWN)),
                        KeyCode::ArrowLeft if focus => cmds.push(Cmd::Key(keys::LEFT)),
                        KeyCode::ArrowRight if focus => cmds.push(Cmd::Key(keys::RIGHT)),
                        KeyCode::ArrowUp if menu => cmds.push(Cmd::MenuMove(-1)),
                        KeyCode::ArrowDown if menu => cmds.push(Cmd::MenuMove(1)),
                        _ => {}
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let now = to_picture((position.x as f32, position.y as f32));
                    if let Some(prev) = self.drag {
                        // a middle drag takes the ground under the cursor with it
                        let (view, proj) = game.view();
                        let size = (WIDTH as f32, HEIGHT as f32);
                        if let (Some(a), Some(b)) = (pick::floor_at(&view, &proj, prev, size), pick::floor_at(&view, &proj, now, size)) {
                            game.camera.target[0] += a[0] - b[0];
                            game.camera.target[1] += a[1] - b[1];
                            game.keep_on_lot();
                        }
                        self.drag = Some(now);
                    }
                    self.cursor = (position.x as f32, position.y as f32);
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    let at = to_picture(self.cursor);
                    match (button, state) {
                        (MouseButton::Left, ElementState::Pressed) => cmds.push(Cmd::Click(at.0, at.1)),
                        (MouseButton::Right, ElementState::Pressed) => cmds.push(Cmd::Close),
                        (MouseButton::Middle, ElementState::Pressed) => self.drag = Some(at),
                        (MouseButton::Middle, ElementState::Released) => self.drag = None,
                        _ => {}
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    self.wheel += match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                    };
                }
                _ => {}
            }
        }
        cmds
    }

    /// The camera's moves for a frame of `dt` seconds.
    fn camera(&mut self, game: &mut Game, dt: f32) {
        // a wheel notch up is in
        let wheel = std::mem::take(&mut self.wheel) * WHEEL_ZOOM;
        let h = |a: KeyCode, b: KeyCode| self.key(a) - self.key(b);
        let orbit = (h(KeyCode::KeyE, KeyCode::KeyQ) + dead(self.right_stick.0)).clamp(-1.0, 1.0);
        let zoom = h(KeyCode::Equal, KeyCode::Minus) + h(KeyCode::NumpadAdd, KeyCode::NumpadSubtract) + self.triggers.1 - self.triggers.0;
        let right = (h(KeyCode::KeyD, KeyCode::KeyA) + h(KeyCode::ArrowRight, KeyCode::ArrowLeft) + dead(self.left_stick.0)).clamp(-1.0, 1.0);
        let forward = (h(KeyCode::KeyW, KeyCode::KeyS) + h(KeyCode::ArrowUp, KeyCode::ArrowDown) + dead(self.left_stick.1)).clamp(-1.0, 1.0);
        game.move_camera(dt, orbit, zoom.clamp(-1.0, 1.0), wheel, (right, forward));
    }
}

/// The pad's events as commands, and its sticks in the input.
fn pad_events(pad: &mut Option<gilrs::Gilrs>, input: &mut Input, game: &Game) -> Vec<Cmd> {
    use gilrs::{Axis, Button, EventType};
    let mut cmds = Vec::new();
    let Some(g) = pad else { return cmds };
    while let Some(ev) = g.next_event() {
        match ev.event {
            EventType::AxisChanged(axis, v, _) => match axis {
                Axis::LeftStickX => input.left_stick.0 = v,
                Axis::LeftStickY => input.left_stick.1 = v,
                Axis::RightStickX => input.right_stick.0 = v,
                _ => {}
            },
            EventType::ButtonChanged(Button::LeftTrigger2, v, _) => input.triggers.0 = v,
            EventType::ButtonChanged(Button::RightTrigger2, v, _) => input.triggers.1 = v,
            EventType::ButtonPressed(b, _) => match b {
                Button::Start => cmds.push(Cmd::TogglePause),
                Button::LeftTrigger => cmds.push(Cmd::Speed(game.speed.saturating_sub(1).max(1))),
                Button::RightTrigger => cmds.push(Cmd::Speed((game.speed + 1).min(3))),
                Button::North | Button::West => cmds.push(Cmd::NextSim),
                Button::East => cmds.push(Cmd::Close),
                Button::DPadUp if game.focus() => cmds.push(Cmd::Key(keys::UP)),
                Button::DPadDown if game.focus() => cmds.push(Cmd::Key(keys::DOWN)),
                Button::DPadLeft if game.focus() => cmds.push(Cmd::Key(keys::LEFT)),
                Button::DPadRight if game.focus() => cmds.push(Cmd::Key(keys::RIGHT)),
                Button::DPadUp => cmds.push(Cmd::MenuMove(-1)),
                Button::DPadDown => cmds.push(Cmd::MenuMove(1)),
                Button::South => {
                    if game.menu.is_some() || game.focus() {
                        cmds.push(Cmd::Confirm);
                    } else {
                        cmds.push(Cmd::Click(WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    cmds
}

/// The meshes and textures a scene made that are not on the GPU yet, uploaded, and the frame
/// with the renderer's own ids (as `view` does).
fn upload(builder: &mut Builder, renderer: &mut Renderer, mesh_ids: &mut Vec<housefire_render::frame::MeshId>, texture_ids: &mut Vec<housefire_render::frame::TextureId>) -> Frame {
    let scene = &mut builder.scene;
    if scene.meshes.len() > mesh_ids.len() {
        let ids = renderer.load_meshes(&scene.meshes[mesh_ids.len()..]);
        mesh_ids.extend(ids);
    }
    for levels in &scene.textures[texture_ids.len()..] {
        texture_ids.push(renderer.load_texture(levels));
    }
    renderer.set_transient_meshes(&scene.transient);
    let mut frame = Frame { camera: scene.frame.camera, draws: std::mem::take(&mut scene.frame.draws), backdrop: scene.frame.backdrop };
    for d in &mut frame.draws {
        if d.mesh.0 & TRANSIENT_MESH == 0 {
            d.mesh = mesh_ids[d.mesh.0 as usize];
        }
        for l in d.material.layers.iter_mut().flatten() {
            l.texture = texture_ids[l.texture.0 as usize];
        }
    }
    frame
}

fn main() -> ExitCode {
    let Some(args) = args() else {
        eprintln!("usage: play LOG ISO --card CARD [--lot N --hood CARD] [--seed N] [--script FILE] [--headless] [--ticks N] [--shot TICK FILE] [--mute] [--wav FILE] [--device]");
        return ExitCode::from(2);
    };
    match run(args) {
        Ok(ok) => {
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("play: {e}");
            ExitCode::FAILURE
        }
    }
}

fn first_camera(log: &Log) -> Result<Camera, String> {
    let c = log.cams.first().ok_or("the log has no camera records")?;
    // the wanted camera at the log's start (the live one is still easing to it)
    let w = c.wanted;
    Ok(Camera { target: [w[0], w[1], w[2]], distance: w[3], rotation: w[4], tilt: w[5] })
}

/// The sound of the run: through the device in a window (silent, said once, if none opens), into a
/// WAV in a headless run that has `--wav`, none with `--mute`. The bank and the events live as
/// long as the process (the mixer thread holds them).
fn make_sound(disc: &Disc, args: &Args) -> Option<Sound> {
    if args.mute || (args.headless && args.wav.is_none() && !args.device) {
        return None;
    }
    let (Some(bank), Some(events)) = (housefire::sound::SoundBank::open(disc), housefire::sound_events::SoundEvents::open(disc)) else {
        eprintln!("play: the disc's sound archives do not read; going on silent");
        return None;
    };
    let (bank, events): (&'static _, &'static _) = (Box::leak(Box::new(bank)), Box::leak(Box::new(events)));
    match &args.wav {
        Some(path) if args.headless && !args.device => Some(Sound::wav(bank, events, args.seed ^ SOUND_SEED, path.clone())),
        _ => match Sound::device(bank, events, args.seed ^ SOUND_SEED) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("play: no sound ({e}); going on silent");
                None
            }
        },
    }
}

fn run(args: Args) -> Result<bool, String> {
    let log = read_log(&args.log).ok_or("cannot read the log")?;
    let disc = Disc::open(&args.iso).map_err(|_| "cannot read the disc image")?;
    let lib = Library::bustin_out(&disc);
    let mut start = Start::read(&disc, args.card.as_deref(), args.lot)?;
    if let Some(p) = &args.hood {
        start.hood_from(p)?;
    }
    let house = std::env::var("HF_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| start.globals().get(10).map_or(HOME_HOUSE, |&n| (n.max(1)) as u32));
    let catalog = housefire::anim::Catalog::read(&disc, house);
    let with = RunWith { card_lot: start.card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: (!log.tracks.is_empty()).then(|| catalog.clone()) };
    let mut alone = Alone::new(&log, &lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, with, args.seed).ok_or("the log does not start with the lot loaded")?;
    alone.set_player(true);
    let camera = first_camera(&log)?;
    let names = engine_scene::save_file(&disc, args.card.as_deref().map(std::path::Path::new)).map(|ngh| housefire::figure::sim_names(&ngh)).unwrap_or_default();
    let interface = AptAssets::new(&disc);
    let font = interface.fonts.iter().find(|(n, _)| housefire::shader::name_hash(n) == INTERFACE_FONT).map(|(_, f)| f.clone()).ok_or("the interface's font")?;
    let ui = Ui::new(interface.flashes.clone());
    let mut game = Game { alone, sim_frames: Default::default(), first_shown: 0, frames_ready: 0, animators: sims::Animators::default(), figures: false, lib: &lib, camera, active: None, target: None, menu: None, dialog: None, names, speed: 1, last_speed: 1, quit: false, expects: Vec::new(), sound: None, ui, hud: Hud::new(1), font, texts: Vec::new(), frames: 0, paused_frames: 0 };
    game.sound = make_sound(&disc, &args);
    game.alone.set_sound(game.sound.is_some());
    if let Some(s) = &mut game.sound {
        s.music(Some(LOAD_MUSIC));
    }
    game.next_sim();
    game.keep_on_lot();
    if std::env::var_os("PLAY_DEBUG").is_some() {
        for o in game.alone.world().objects().filter(|o| o.person) {
            println!("debug: sim {} {} pd32 {:?} at ({}, {}) queue {:?} current {:?}", o.id(), game.sim_name(o.id()), o.person_data.get(32), o.x16, o.y16, o.queue.iter().map(|q| (q.objects, q.tree)).collect::<Vec<_>>(), (o.current.objects, o.current.tree));
        }
    }

    let mut script: Vec<(u32, Cmd)> = Vec::new();
    if let Some(p) = &args.script {
        for (n, line) in std::fs::read_to_string(p).map_err(|e| format!("the script: {e}"))?.lines().enumerate() {
            if line.split('#').next().is_some_and(|l| l.trim().is_empty()) {
                continue;
            }
            script.push(parse_line(line).ok_or_else(|| format!("script line {}: {line}", n + 1))?);
        }
        script.sort_by_key(|s| s.0);
    }
    let script_end = script.last().map_or(0, |s| s.0);
    let mut next_script = 0;
    let limit = args.ticks.or_else(|| (args.headless && args.script.is_some()).then_some(script_end + SCRIPT_TAIL));

    let want_gpu = !args.headless || args.shot.is_some();
    let mut event_loop = if args.headless { None } else { Some(EventLoop::new().map_err(|e| e.to_string())?) };
    let mut app = App::new("House Fire", WIDTH * WINDOW_SCALE, HEIGHT * WINDOW_SCALE);
    let gpu = if !want_gpu {
        None
    } else if let Some(el) = &mut event_loop {
        let display = el.display_handle().map_err(|e| e.to_string())?.as_raw();
        let mut extensions = window::surface_extensions(display)?;
        extensions.extend(window::layer_extensions(&unsafe { ash::Entry::load() }.map_err(|e| format!("no Vulkan loader: {e}"))?));
        Some(Gpu::open(std::env::var_os("HF_VALIDATE").is_some(), &extensions, &[ash::khr::swapchain::NAME.as_ptr()])?)
    } else {
        Some(Gpu::new(std::env::var_os("HF_VALIDATE").is_some())?)
    };
    let mut swapchain = None;
    if let (Some(el), Some(gpu)) = (&mut event_loop, &gpu) {
        while app.window.is_none() {
            el.pump_app_events(Some(Duration::ZERO), &mut app);
        }
        let w = app.window.as_ref().expect("a window");
        let display = el.display_handle().map_err(|e| e.to_string())?.as_raw();
        let surface = window::make_surface(gpu, display, w.window_handle().map_err(|e| e.to_string())?.as_raw())?;
        swapchain = Some(Swapchain::new(gpu, surface, w.inner_size())?);
    }
    let pool = match (&gpu, &swapchain) {
        (Some(gpu), Some(_)) => Some(unsafe { gpu.device.create_query_pool(&vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2), None) }.map_err(|e| e.to_string())?),
        _ => None,
    };

    // the picture's parts, only where there is a GPU
    let assets = Assets::new(&disc);
    let lot_art = if want_gpu { Some(Lot::new(&disc, house as usize).ok_or("the lot's art")?) } else { None };
    let mut renderer = gpu.as_ref().map(|g| Renderer::new(g, WIDTH, HEIGHT));
    let mut staged = None;
    if let Some(lot) = &lot_art {
        let ngh = engine_scene::save_file(&disc, args.card.as_deref().map(std::path::Path::new)).ok_or("the save")?;
        let rigs = housefire::figure::Rigs::read(&disc).ok_or("the skeletons")?;
        let cast = housefire::figure::Cast::read(&disc, &ngh).ok_or("the part tables")?;
        let drawn_ticks: std::collections::BTreeSet<u32> = log.strips.iter().map(|s| s.tick).collect();
        let log_scene = drawn_ticks.first().map(|&t| assets.scene(&log, t));
        let skins = log_scene.as_ref().map_or_else(SimSkins::default, SimSkins::from_scene).with_disc(&disc);
        staged = Some((Builder::new(&assets, lot), rigs, cast, skins));
        game.figures = true;
    }
    let mut painter = Painter::new(&interface);
    let mut input = Input::default();
    let mut pad = if args.headless { None } else { gilrs::Gilrs::new().ok() };
    let (mut mesh_ids, mut texture_ids) = (Vec::new(), Vec::new());
    let frame_length = Duration::from_secs_f64(1.0 / FRAMES_PER_SECOND);
    let begin = Instant::now();
    let mut frames = 0u32;
    let mut shot_done = false;

    'frames: loop {
        if !args.headless {
            let due = begin + frame_length * frames;
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait.saturating_sub(SPIN));
                while Instant::now() < due {
                    std::hint::spin_loop();
                }
            }
        }
        if let Some(el) = &mut event_loop {
            el.pump_app_events(Some(Duration::ZERO), &mut app);
            let size = app.window.as_ref().map_or(PhysicalSize::new(WIDTH, HEIGHT), |w| w.inner_size());
            let events = std::mem::take(&mut app.events);
            let mut cmds = input.events(events, size, &mut game);
            cmds.extend(pad_events(&mut pad, &mut input, &game));
            for c in cmds {
                game.apply(c);
            }
            input.camera(&mut game, 1.0 / FRAMES_PER_SECOND as f32);
            if app.closed {
                break;
            }
        }
        // the script's lines due now, and the ticks of the picture: in a window, enough for its
        // frames (the speed's, a frame a picture at the normal speed); headless, a tick a picture
        let frames_now = (!args.headless).then_some(game.speed);
        let mut ran = 0;
        loop {
            while next_script < script.len() && script[next_script].0 <= game.clock() {
                let c = script[next_script].1.clone();
                next_script += 1;
                game.apply(c);
            }
            if game.quit || limit.is_some_and(|l| game.clock() >= l) {
                break 'frames;
            }
            // (the frame of a shot is drawn before its tick runs)
            if args.shot.as_ref().is_some_and(|s| s.0 == game.clock()) && !shot_done {
                break;
            }
            match frames_now {
                Some(want) if game.frames_ready >= want => break,
                None if ran >= 1 => break,
                _ => {}
            }
            game.tick();
            ran += 1;
            // a dialog open stops the frame's ticks short for the player to answer
            if game.dialog.is_some() {
                break;
            }
        }
        frames += 1;
        let show = frames_now.map_or(game.frames_ready, |want| want.min(game.frames_ready));
        game.frames_ready -= show;
        let shown_at = game.clock();
        if ran == 0 && game.speed == 0 {
            game.paused_frames += 1;
        }
        game.frame_ui();
        if let Some(s) = &mut game.sound {
            s.flush();
        }

        // the picture
        if let (Some((builder, rigs, cast, skins)), Some(renderer), Some(lot)) = (&mut staged, &mut renderer, &lot_art) {
            let tick = game.alone.ticks();
            let wardrobe = Wardrobe { rigs, cast, catalog: &catalog, skins, models: &lot.models, shaders: &lot.shaders };
            let mut figures = sims::figures_of(game.alone.world(), cast, &[]);
            // the Sims' animators: stepped by the picture's frames
            if let Some(c) = game.alone.catalog() {
                for _ in 0..show {
                    let Some((held, tick)) = game.sim_frames.front() else { break };
                    game.animators.step_frame(c, game.alone.world(), cast, tick, game.first_shown);
                    game.first_shown += 1;
                    if game.first_shown >= *held {
                        game.sim_frames.pop_front();
                        game.first_shown = 0;
                    }
                }
                game.animators.apply(c, &mut figures);
            }
            builder.build_with(game.alone.world(), tick, game.view(), Some((&wardrobe, &figures, None)));
            let frame = upload(builder, renderer, &mut mesh_ids, &mut texture_ids);
            // the interface: the movies' pieces and the game's own lines of text over the scene
            let list = game.ui.display_list();
            let pass = renderer.interface();
            let mut drawing = painter.draw(pass, &list);
            for run in &game_text::native_lines(&interface, &game.texts) {
                painter.text(pass, &mut drawing, run);
            }
            pass.set(&drawing.vertices, &drawing.batches);
            renderer.show_interface(true);
            if let Some((at, path)) = &args.shot
                && *at == shown_at
                && !shot_done
            {
                let picture = renderer.render(&frame);
                window::write_png(path, picture.width, picture.height, &picture.rgba);
                println!("play: shot of tick {tick} saved ({} draws)", frame.draws.len());
                shot_done = true;
                if args.headless {
                    break;
                }
            }
            if let (Some(gpu), Some(sc), Some(pool)) = (&gpu, &swapchain, pool) {
                let (image, _) = unsafe { sc.loader.acquire_next_image(sc.chain, u64::MAX, sc.acquire, vk::Fence::null()) }.map_err(|e| format!("acquire: {e}"))?;
                let release = sc.release[image as usize];
                renderer.draw(&frame, Some(&Present { image: sc.images[image as usize], extent: sc.extent, wait: sc.acquire, signal: release, timestamps: pool }));
                let present = vk::PresentInfoKHR::default().wait_semaphores(std::slice::from_ref(&release)).swapchains(std::slice::from_ref(&sc.chain)).image_indices(std::slice::from_ref(&image));
                unsafe { sc.loader.queue_present(gpu.queue, &present) }.map_err(|e| format!("present: {e}"))?;
            }
        }
    }

    let mut ok = true;
    for e in &game.expects {
        match e.seen {
            Some(t) if t <= e.deadline => println!("play: expect sim {} tree {}: ok, entered at tick {t} ({} ticks after the command)", e.sim, e.tree, t - e.asked),
            _ => {
                println!("play: expect sim {} tree {}: FAILED, not entered within {} ticks of tick {}", e.sim, e.tree, e.deadline - e.asked, e.asked);
                ok = false;
            }
        }
    }
    for id in game.household() {
        if let Some(o) = game.alone.world().find_object(id) {
            println!("play: sim {id} ends at ({:.2}, {:.2}), queue {}, stack {:?}", o.x16 as f32 / 16.0, o.y16 as f32 / 16.0, o.queue.len(), game.alone.frames_of(id).iter().map(|f| f.0).collect::<Vec<_>>());
        }
    }
    println!("play: {} frames, {} ticks", frames, game.alone.ticks());
    if let Some(s) = game.sound.take() {
        s.finish()?;
    }
    drop(staged);
    if let Some(gpu) = &gpu {
        unsafe { gpu.device.device_wait_idle().ok() };
        if let Some(pool) = pool {
            unsafe { gpu.device.destroy_query_pool(pool, None) };
        }
        if let Some(sc) = swapchain {
            sc.destroy(gpu);
        }
    }
    drop(renderer);
    Ok(ok)
}
