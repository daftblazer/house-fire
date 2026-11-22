//! The buy and build screens as the game runs them around the movies (`docs/assets.md`, "Buy and
//! Build"): the sections list, the item list with its info dialog, and place mode, the calls the
//! game makes on the wrapper for each, the lines of text it draws and what the player's presses
//! do.
//!
//! The game reads the pad itself: a press on one of these screens is not a key to the movie (but
//! the arrows) and moves the game to another screen after a delay of its own (`delay`). The
//! screen never changes the world: placing hands the caller a `Purchase` (`Event::Purchase`),
//! and the price is held back from the funds shown until the world has it (`Event::Reserve`).

use super::font::Font;
use super::pause::{Game, Press, dpad, draw_button, live, make, title_width, width, Face, HALF_BIG, HALF_SMALL, SECTION_TITLE_Y};
use crate::catalog::{BUY_CATEGORIES, Purchase};
use crate::hud::{Call, SCREEN, Text, line};

/// Which of the two screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Buy,
    Build,
}

/// The screens of one kind: the sections list, the item list, and place mode (buy: placing the
/// object, then the object placed; build: choosing a starting point, then the wall's preview).
/// `Between` is the short while after place mode has gone and before the list is back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Sections,
    Items,
    Place,
    Placed,
    Start,
    Preview,
    Between,
}

/// A change of screen the game makes (a press brings it after `delay`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: Screen,
    pub to: Screen,
}

/// What the screen asks of its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The price of the object being placed is taken from the funds shown.
    Reserve(i32),
    /// The placement is given up: the price is back.
    Release,
    /// The object is placed.
    Purchase(Purchase),
}

/// The game frames from a press to the change of screen it brings, by kind and move. Measured
/// (buy-a.log; the frame of the pad's change against the frame of the calls): buy 2019 to 2027,
/// 2102 to 2110, 2185 at once, 2268 at once then 2276, 2323 to 2331; build 2683 at once, 2766 to
/// 2774, 2849 and 2932 at once, 2988 at once then 2996, 3043 to 3051.
fn delay(kind: Kind, mv: Move) -> u32 {
    use Screen::*;
    match (kind, mv.from, mv.to) {
        (Kind::Buy, Sections, Items) | (_, Items, Sections) | (_, Items, Place | Start) | (_, Between, Items) => 8,
        _ => 0,
    }
}

/// What the pad's A and B do on a screen: the screen they lead to.
fn target(kind: Kind, screen: Screen, key: u32) -> Option<Screen> {
    use super::ui::keys::{A, B};
    use Screen::*;
    Some(match (kind, screen, key) {
        (_, Sections, A) => Items,
        (_, Items, A) => if kind == Kind::Buy { Place } else { Start },
        (_, Items, B) => Sections,
        (Kind::Buy, Place, A) => Placed,
        (Kind::Buy, Place | Placed, B) => Between,
        (Kind::Build, Start, A) => Preview,
        (Kind::Build, Start, B) => Between,
        (Kind::Build, Preview, B) => Start,
        _ => return None,
    })
}

/// The screens' places (fractions of the screen), measured in buy-a.log.
mod at {
    /// The item list's info dialog: the name's left and the price's centre (the same y: the
    /// face's top), the description's left, first top, the pitch to the second line and the
    /// pitch after it (20 pixels), the first motive lozenge's centre and top.
    pub const NAME_X: f32 = 0.165625;
    pub const NAME_Y: f32 = 0.15964286;
    pub const PRICE_CENTRE: f32 = 0.871875;
    pub const DESC_TOP: f32 = 0.3067857;
    pub const DESC_FIRST_PITCH: f32 = 0.04447243;
    pub const DESC_PITCH: f32 = 20.0 / 448.0;
    pub const LOZENGE: (f32, f32) = (0.719, 0.5230844);
    /// The two buttons of the info dialog (the game's `drawButton` places).
    pub const BUY: (f32, f32) = (0.35, 0.75);
    pub const BACK: (f32, f32) = (0.7, 0.75);
    /// The build list's lines: the item's name (left), its price, the y of both (the face's top),
    /// and the buttons' y (the sections' are the pause screen's, 0.905).
    pub const BUILD_NAME_X: f32 = 0.2050625;
    pub const BUILD_PRICE_X: f32 = 0.8856439;
    pub const BUILD_ITEM_Y: f32 = 0.76847726;
    pub const BUILD_BUTTONS_Y: &str = ".7";
    /// Place mode's title (its top), the buttons' columns and rows, and the price label.
    pub const TITLE_Y: f32 = 0.05582387;
    pub const ROW_LOW: f32 = 0.910446;
    pub const ROW_HIGH: f32 = 0.81;
    pub const BUY_PRICE: (f32, f32) = (0.105, 0.695);
    pub const BUILD_PRICE: (f32, f32) = (0.856, 0.72);
}

/// Half a place-mode button label's glyph box: the small size's by 13 over 14 (the labels' tops,
/// 0.89264 for a y of 0.910446).
const HALF_PLACE: f32 = HALF_SMALL * 13.0 / 14.0;

/// The width of the wrapped description's lines, in glyph columns (a fit: the log's 13 lines
/// hold for any limit from 459 to 480).
const DESC_WRAP: f32 = 470.0;

/// The lines of the description the info window shows before it needs its scroll arrow (any
/// number from 1 to 12 holds for the log's 13-line text; unverified).
const INFO_LINES: usize = 12;

/// The motive lozenges beyond the first are drawn this far right of the one before it (only
/// one is in the log: a guess).
const LOZENGE_PITCH: f32 = 0.06;

/// The price of the build screen's wall tool and the estimate its preview shows (buy-a.log,
/// frames 2683 and 2849).
const WALL_PRICE: i32 = 70;

/// Place mode's label widths: the string, its width in pixels and the width the game gives
/// `drawButton` for it (fractions of the screen, as logged). Measured: the size words of the
/// log's draws and the calls' arguments (frames 2110, 2185, 2774, 2849).
const LABELS: &[(&str, f32, f32)] = &[
    ("PLACE", 47.7121, 0.067095),
    ("ROTATE", 60.1402, 0.084572),
    ("WALLS Up/Down", 122.7424, 0.172607),
    ("SELL", 33.1174, 0.046571),
    ("CANCEL", 60.6818, 0.085334),
    ("GRAB", 40.1591, 0.056474),
    ("BACK", 41.2424, 0.057997),
    ("START Building", 109.1705, 0.153521),
    ("START Tear Down", 131.3295, 0.205202),
    ("BUILD", 40.6705, 0.057193),
    ("PREVIEW Room", 110.3143, 0.172366),
    ("$ 79", 33.7727, 0.052770),
    ("BUY", 28.2726, 0.044176),
    ("≈ $ 70", 54.2576, 0.140000),
];

/// The place-mode face's width over the big one's (BACK: 41.2424 against 45.0303).
const PLACE_OVER_BIG: f32 = 41.2424 / 45.0303;

/// The width the game gives `drawButton` for the item list's BACK (41.2424 pixels: the place
/// face's, frame 2027).
const ITEM_BACK_WIDTH: f32 = 0.064441;

/// The titles of place mode, as the movie reads them (`place_title_bar_width`).
const PLACE_TITLES: &[(&str, f32)] = &[("Place Object", 0.154545), ("Choose Starting Point", 0.261707), ("Build Wall", 0.126823)];

/// The build screen's section names (positions in `LiveModeMenuUIStrings`): "Walls" is the
/// log's, the rest are the other names the table has for what the game builds (unverified).
const BUILD_SECTIONS: [usize; 8] = [live::WALLS, 62, 178, 68, 174, 67, 111, 27];

/// What the info window / list shows for the selected item.
struct Shown {
    name: String,
    price: i32,
    description: String,
    effects: Vec<(usize, i32)>,
}

/// One of the two screens' state.
#[derive(Clone, Debug)]
pub struct Shop {
    kind: Kind,
    screen: Screen,
    section: usize,
    item: usize,
    /// A change of screen is waiting for its frame.
    moving: bool,
    /// The list's item is not shown (the build list drops it when a press is on its way).
    item_hidden: bool,
    /// The movie has sent `motivesUp`: the lozenges' amounts are drawn.
    lozenges: bool,
    events: Vec<Event>,
    /// The frame the screen was entered at (its per-frame calls start the frame after).
    entered: u32,
}

impl Shop {
    pub fn new(kind: Kind) -> Shop {
        Shop { kind, screen: Screen::Sections, section: 0, item: 0, moving: false, item_hidden: false, lozenges: false, events: Vec::new(), entered: 0 }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The section the screen is on.
    pub fn section(&self) -> usize {
        self.section
    }

    /// Whether the pause screen's own back (to the main row) is what B does here.
    pub fn leaves(&self) -> bool {
        self.screen == Screen::Sections && !self.moving
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn set_lozenges(&mut self, on: bool) {
        self.lozenges = on;
    }

    /// The section the movie says it is on.
    pub fn set_section(&mut self, section: usize) {
        self.section = section;
        self.item = 0;
    }

    /// A press of A or B: the changes of screen it brings, with their frames (the screen's
    /// presses while one is on its way are not taken).
    pub fn key(&mut self, key: u32, frame: u32) -> Vec<(u32, Press)> {
        let Some(to) = target(self.kind, self.screen, key).filter(|_| !self.moving) else { return Vec::new() };
        let mv = Move { from: self.screen, to };
        self.moving = true;
        let at = frame + delay(self.kind, mv);
        let mut out = Vec::new();
        if self.kind == Kind::Build && mv.from == Screen::Items {
            // the build list drops its item and the game turns the arrows off as the press comes
            out.push((frame, Press::ShopNow(mv)));
        }
        out.push((at, Press::Shop(mv)));
        out
    }

    /// What the game does the frame a press is on its way (the build list).
    pub fn now(&mut self, mv: Move) -> Vec<Call> {
        self.item_hidden = true;
        if mv.to == Screen::Start { dpad("0", "0", "0", "0") } else { Vec::new() }
    }

    /// A change of screen is due: the calls the game makes, and the change that follows it (the
    /// pause after place mode ends by itself).
    pub fn apply(&mut self, mv: Move, frame: u32, game: &Game, font: &Font, cursor: (f32, f32)) -> (Vec<Call>, Vec<(u32, Press)>) {
        let mut follow = Vec::new();
        self.entered = frame;
        self.moving = false;
        self.screen = mv.to;
        self.item_hidden = false;
        let mut calls = self.entry(mv, game, font);
        match mv.to {
            Screen::Place => {
                if let Some(s) = self.shown(game) {
                    self.events.push(Event::Reserve(s.price));
                }
            }
            Screen::Placed => {
                if let Some(item) = self.item_of(game) {
                    let at = (cursor.0.floor() as i32 * 16 + 8, cursor.1.floor() as i32 * 16 + 8);
                    self.events.push(Event::Purchase(Purchase { guid: item, price: self.reserved_price(game), at, level: 1, facing: 0 }));
                }
            }
            Screen::Between => {
                if mv.from == Screen::Place {
                    self.events.push(Event::Release);
                }
                let next = Move { from: Screen::Between, to: Screen::Items };
                self.moving = true;
                follow.push((frame + delay(self.kind, next), Press::Shop(next)));
            }
            Screen::Items => self.lozenges = false,
            _ => {}
        }
        if mv.to == Screen::Sections {
            self.lozenges = false;
        }
        calls.shrink_to_fit();
        (calls, follow)
    }

    fn reserved_price(&self, game: &Game) -> i32 {
        self.shown(game).map_or(0, |s| s.price)
    }

    fn item_of(&self, game: &Game) -> Option<u32> {
        match self.kind {
            Kind::Buy => game.catalog.items(self.section).get(self.item).map(|i| i.guid),
            Kind::Build => None,
        }
    }

    fn shown(&self, game: &Game) -> Option<Shown> {
        match self.kind {
            Kind::Buy => game.catalog.items(self.section).get(self.item).map(|i| Shown { name: i.name.clone(), price: i.price, description: i.description.clone(), effects: i.effects.clone() }),
            Kind::Build => (self.section == 0).then(|| Shown { name: game.text.live_at(173).to_string(), price: WALL_PRICE, description: String::new(), effects: Vec::new() }),
        }
    }

    /// The width of a place-mode label in pixels, and what the game passes `drawButton` for it.
    fn label(font: &Font, s: &str) -> (f32, f32) {
        LABELS.iter().find(|l| l.0 == s).map_or_else(
            || {
                let px = width(font, s, Face::Big) * PLACE_OVER_BIG;
                (px, px * 0.9 / SCREEN.0)
            },
            |l| (l.1, l.2),
        )
    }

    fn hide_all() -> Vec<Call> {
        ["accept", "decline", "walls", "alt2", "flipper1", "flipper2", "rotate", "background"].map(|b| make("hideButton", &[b])).to_vec()
    }

    /// The text of a string of the live-mode table.
    fn s(game: &Game, k: usize) -> &str {
        game.text.live_at(k)
    }

    /// The calls of a change of screen.
    fn entry(&self, mv: Move, game: &Game, font: &Font) -> Vec<Call> {
        use Screen::*;
        let t = |k| Self::s(game, k);
        let cancel = game.text.global_at(14);
        let (back, select) = (t(live::BACK), t(live::SELECT));
        let main_buttons = |y: &str| -> Vec<Call> {
            let w = |l: &str| format!("{:.6}", title_width(font, l));
            vec![
                Call { name: "drawButton", args: vec!["accept".into(), "0.400000".into(), y.into(), w(select)] },
                Call { name: "drawButton", args: vec!["decline".into(), "0.700000".into(), y.into(), w(back)] },
            ]
        };
        let arrows = || self.arrows(game, font);
        let place_buttons = |kind: Kind, to: Screen, price_label: Option<&str>| -> Vec<Call> {
            let b = |name: &'static str, x: f32, y: f32, label: &str| draw_button(name, (x, y), Self::label(font, label).1);
            let mut c = Self::hide_all();
            c.push(make("placeTitleBarWidth", &[]));
            match (kind, to) {
                (Kind::Buy, Place) => {
                    let p = price_label.unwrap_or("");
                    c.extend([b("accept", 0.31, at::ROW_LOW, t(104)), b("rotate", 0.58, at::ROW_HIGH, t(126)), b("walls", 0.58, at::ROW_LOW, t(176)), b("alt2", 0.86, at::ROW_HIGH, t(140)), b("decline", 0.86, at::ROW_LOW, cancel), b("background", at::BUY_PRICE.0, at::BUY_PRICE.1, p)]);
                }
                (Kind::Buy, _) => {
                    c.extend([b("accept", 0.31, at::ROW_LOW, t(71)), b("walls", 0.58, at::ROW_LOW, t(176)), b("decline", 0.86, at::ROW_LOW, back), make("hideButton", &["background"])]);
                }
                (Kind::Build, Start) => {
                    c.extend([b("accept", 0.325, at::ROW_LOW, t(144)), b("alt2", 0.605, at::ROW_HIGH, t(150)), b("walls", 0.605, at::ROW_LOW, t(176)), b("decline", 0.89, at::ROW_LOW, back), make("hideButton", &["background"])]);
                }
                (Kind::Build, _) => {
                    let p = price_label.unwrap_or("");
                    c.extend([b("accept", 0.325, at::ROW_LOW, t(live::BUILD)), b("flipper2", 0.605, at::ROW_HIGH, t(112)), b("walls", 0.605, at::ROW_LOW, t(176)), b("decline", 0.89, at::ROW_LOW, back), b("background", at::BUILD_PRICE.0, at::BUILD_PRICE.1, p)]);
                }
            }
            c
        };
        let price_text = |kind: Kind| -> String {
            let price = self.shown(game).map_or(0, |s| s.price);
            if kind == Kind::Buy { format!("$ {price}") } else { format!("≈ $ {price}") }
        };
        match (self.kind, mv.from, mv.to) {
            (Kind::Buy, Sections, Items) => {
                let mut c = vec![make("hideBuildBuySections", &[]), make("showBuildBuyItems", &[]), make("hideButton", &["accept"]), draw_button("decline", at::BACK, ITEM_BACK_WIDTH)];
                c.extend(dpad("0", "0", "1", "1"));
                c.extend([make("showInfoDialog", &[]), draw_button("accept", at::BUY, Self::label(font, t(45)).1)]);
                c.extend(arrows());
                c
            }
            (Kind::Buy, Between, Items) => {
                let mut c = vec![make("showBuildBuyItems", &[]), make("hideButton", &["accept"]), draw_button("decline", at::BACK, ITEM_BACK_WIDTH)];
                c.extend(dpad("0", "0", "1", "1"));
                c.extend([make("showInfoDialog", &[]), draw_button("accept", at::BUY, Self::label(font, t(45)).1)]);
                c.extend(arrows());
                c
            }
            (Kind::Buy, Items, Place) => {
                let mut c = dpad("0", "0", "0", "0");
                c.extend([make("hideBuildBuySections", &[]), make("hideBuildBuyItems", &[]), make("placeTitleBarWidth", &[])]);
                c.extend(dpad("0", "0", "0", "0"));
                c.extend([make("hideInfoDialog", &[]), make("showPlaceTitleBar", &[])]);
                let mut rest = place_buttons(Kind::Buy, Place, Some(&price_text(Kind::Buy)));
                // (the hides come before the title bar's width call in this one)
                rest.retain(|c| c.name != "placeTitleBarWidth");
                c.extend(rest);
                c
            }
            (Kind::Buy, Place, Placed) => {
                let mut c = vec![make("hidePlaceTitleBar", &[])];
                let mut rest = place_buttons(Kind::Buy, Placed, None);
                rest.retain(|c| c.name != "placeTitleBarWidth");
                c.extend(rest);
                c
            }
            (Kind::Buy, _, Between) => {
                let mut c = Self::hide_all();
                c.push(make("showPlaceTitleBar", &[]));
                c
            }
            (Kind::Buy, Items, Sections) | (Kind::Build, Items, Sections) => {
                let mut c = if self.kind == Kind::Buy { vec![make("hideBuildBuyItems", &[]), make("showBuildBuySections", &[])] } else { vec![make("hideBuildBuyItems", &[]), make("hideLongTextBar", &[]), make("showBuildBuySections", &[])] };
                c.extend(main_buttons("0.905000"));
                c.extend(dpad("1", "1", "0", "0"));
                if self.kind == Kind::Buy {
                    c.push(make("hideInfoDialog", &[]));
                }
                c
            }
            (Kind::Build, Sections, Items) => {
                let mut c = vec![make("hideBuildBuySections", &[]), make("showBuildBuyItems", &[]), make("showLongTextBar", &[])];
                c.extend(dpad("0", "0", "1", "1"));
                c.extend(main_buttons(at::BUILD_BUTTONS_Y));
                c
            }
            (Kind::Build, Between, Items) => {
                let mut c = vec![make("showBuildBuyItems", &[]), make("showLongTextBar", &[])];
                c.extend(dpad("0", "0", "1", "1"));
                c.extend(main_buttons(at::BUILD_BUTTONS_Y));
                c
            }
            (Kind::Build, Items, Start) => {
                let mut c = vec![make("hideBuildBuySections", &[]), make("hideBuildBuyItems", &[]), make("hideLongTextBar", &[])];
                c.extend(dpad("0", "0", "0", "0"));
                c.extend(place_buttons(Kind::Build, Start, None));
                c
            }
            (Kind::Build, Preview, Start) => place_buttons(Kind::Build, Start, None),
            (Kind::Build, Start, Preview) => place_buttons(Kind::Build, Preview, Some(&price_text(Kind::Build))),
            (Kind::Build, _, Between) => Self::hide_all(),
            _ => Vec::new(),
        }
    }

    /// The calls the game makes every frame the screen is up: the item list's info window checks
    /// whether its description scrolls (2028 to 2109 and 2277 to 2330).
    pub fn frame_calls(&self, game: &Game, font: &Font, frame: u32) -> Vec<Call> {
        match (self.kind, self.screen) {
            (Kind::Buy, Screen::Items) if frame > self.entered => self.arrows(game, font),
            // the title bar of place mode is shown, then hidden, every frame
            (Kind::Buy, Screen::Place) if frame > self.entered => vec![make("showPlaceTitleBar", &[])],
            (Kind::Buy, Screen::Placed) if frame > self.entered => vec![make("hidePlaceTitleBar", &[])],
            _ => Vec::new(),
        }
    }

    /// The info window's scroll arrows: the one down is shown while the description has lines
    /// beyond the window's (the log's text, 13 lines, shows it and not the one up; a text that
    /// fits is taken to show neither, unverified).
    fn arrows(&self, game: &Game, font: &Font) -> Vec<Call> {
        let lines = self.shown(game).map_or(0, |s| super::pause::wrap(font, &s.description, DESC_WRAP).len());
        let more = lines > INFO_LINES;
        let (down, name) = if more { ("1", "showInfoArrowDown") } else { ("0", "hideInfoArrowDown") };
        vec![make("hideInfoArrowUp", &[]), make("DPadUp", &["0"]), make(name, &[]), make("DPadDown", &[down])]
    }

    /// The answers to the movies' reads on these screens.
    pub fn answers(&self, game: &Game, font: &Font) -> Vec<(String, String)> {
        let f = |v: f32| format!("{v:.6}");
        let mut out = vec![("current_item".to_string(), self.item.to_string())];
        if let Some(s) = self.shown(game) {
            let n = s.effects.len().min(4);
            out.push(("numberOfInfoLozenges".into(), n.to_string()));
            for (k, e) in s.effects.iter().take(4).enumerate() {
                out.push((format!("infoWindowMotive{}", k + 1), e.0.to_string()));
            }
        }
        let title = match (self.kind, self.screen) {
            (_, Screen::Start) => Some("Choose Starting Point"),
            (Kind::Build, Screen::Preview) => Some("Build Wall"),
            (Kind::Buy, Screen::Place | Screen::Placed | Screen::Between) => Some("Place Object"),
            _ => None,
        };
        if let Some(t) = title {
            out.push(("place_title_bar_width".into(), f(Self::place_title_width(t, font))));
        }
        out
    }

    fn place_title_width(s: &str, font: &Font) -> f32 {
        PLACE_TITLES.iter().find(|t| t.0 == s).map_or_else(|| width(font, s, Face::Big) * PLACE_OVER_BIG / SCREEN.0, |t| t.1)
    }

    /// The screen's lines of text, in the order the game draws them (the funds lines are the
    /// caller's, after them). `buttons` is whether the pause screen's two buttons are up; `frame`
    /// the game frame and `opened_at` the frame the section's movie loaded at.
    pub fn texts(&self, game: &Game, font: &Font, buttons: bool, frame: u32, opened_at: u32, out: &mut Vec<Text>) {
        use Screen::*;
        let t = |k| Self::s(game, k);
        let big = |s: &str| width(font, s, Face::Big);
        let h_small = Some(2.0 * HALF_SMALL * SCREEN.1);
        let plain = |out: &mut Vec<Text>, s: &str, x: f32, y: f32, w: f32| out.push(Text { text: s.to_string(), x, y, shadow: false, width: Some(w), height: h_small, plain: true });
        let title = |out: &mut Vec<Text>, s: &str, w: f32, top: f32| out.push(Text { text: s.to_string(), x: 0.5 - w / 2.0, y: top, shadow: false, width: Some(w * SCREEN.0), height: Some(2.0 * HALF_BIG * SCREEN.1), plain: true });
        // a label centred on a place: the button's top is its y less half the glyph box
        let label = |out: &mut Vec<Text>, s: &str, centre: f32, y: f32, px: f32, half: f32| line(out, s, centre - px / SCREEN.0 / 2.0, y - half, Some(px), Some(2.0 * half * SCREEN.1));
        let place_label = |out: &mut Vec<Text>, s: &str, at: (f32, f32)| label(out, s, at.0, at.1, Self::label(font, s).0, HALF_PLACE);
        let section_title = |out: &mut Vec<Text>| match self.kind {
            Kind::Buy => {
                let name = t(BUY_CATEGORIES[self.section.min(BUY_CATEGORIES.len() - 1)].1);
                title(out, name, title_width(font, name), SECTION_TITLE_Y);
            }
            Kind::Build => {
                let name = t(BUILD_SECTIONS[self.section.min(BUILD_SECTIONS.len() - 1)]);
                title(out, name, title_width(font, name), SECTION_TITLE_Y);
            }
        };
        let main_buttons = |out: &mut Vec<Text>, y: f32, first: &str| {
            label(out, first, 0.4, y, big(first), HALF_SMALL);
            label(out, t(live::BACK), 0.7, y, big(t(live::BACK)), HALF_SMALL);
        };
        match (self.kind, self.screen) {
            (_, Sections) => {
                section_title(out);
                let ready = if self.kind == Kind::Buy { buttons } else { frame >= opened_at + super::pause::BUILD_BUTTONS_DELAY };
                if ready {
                    main_buttons(out, 0.905, t(live::SELECT));
                }
            }
            (Kind::Buy, Items) => {
                if let Some(s) = self.shown(game) {
                    line(out, &s.name, at::NAME_X, at::NAME_Y, Some(big(&s.name)), Some(2.0 * HALF_BIG * SCREEN.1));
                    let price = format!("$ {}", s.price);
                    let w = big(&price);
                    line(out, &price, at::PRICE_CENTRE - w / SCREEN.0 / 2.0, at::NAME_Y, Some(w), Some(2.0 * HALF_BIG * SCREEN.1));
                    for (k, l) in super::pause::wrap(font, &s.description, DESC_WRAP).iter().enumerate() {
                        let y = at::DESC_TOP + if k == 0 { 0.0 } else { at::DESC_FIRST_PITCH + (k - 1) as f32 * at::DESC_PITCH };
                        plain(out, l, at::NAME_X, y, big(l));
                    }
                    if self.lozenges {
                        for (k, e) in s.effects.iter().take(4).enumerate() {
                            let n = e.1.to_string();
                            let w = big(&n);
                            let centre = at::LOZENGE.0 + k as f32 * LOZENGE_PITCH;
                            line(out, &n, centre - w / SCREEN.0 / 2.0, at::LOZENGE.1, Some(w), Some(2.0 * HALF_SMALL * SCREEN.1));
                        }
                    }
                }
                label(out, t(45), at::BUY.0, at::BUY.1, big(t(45)), HALF_SMALL);
                label(out, t(live::BACK), at::BACK.0, at::BACK.1, big(t(live::BACK)), HALF_SMALL);
                section_title(out);
            }
            (Kind::Build, Items) => {
                if !self.item_hidden
                    && let Some(s) = self.shown(game)
                {
                    plain(out, &s.name, at::BUILD_NAME_X, at::BUILD_ITEM_Y, big(&s.name));
                    plain(out, &format!("$ {}", s.price), at::BUILD_PRICE_X, at::BUILD_ITEM_Y, 0.0);
                }
                section_title(out);
                main_buttons(out, 0.7, t(live::SELECT));
            }
            (Kind::Buy, Place) => {
                let name = t(105);
                title(out, name, Self::place_title_width(name, font), at::TITLE_Y);
                let price = format!("$ {}", self.shown(game).map_or(0, |s| s.price));
                label(out, &price, at::BUY_PRICE.0, at::BUY_PRICE.1, Self::label(font, &price).0, HALF_SMALL);
                for (s, p) in [(t(104), (0.31, at::ROW_LOW)), (t(126), (0.58, at::ROW_HIGH)), (t(176), (0.58, at::ROW_LOW)), (t(140), (0.86, at::ROW_HIGH)), (game.text.global_at(14), (0.86, at::ROW_LOW))] {
                    place_label(out, s, p);
                }
            }
            (Kind::Buy, Placed) => {
                for (s, p) in [(t(71), (0.31, at::ROW_LOW)), (t(176), (0.58, at::ROW_LOW)), (t(live::BACK), (0.86, at::ROW_LOW))] {
                    place_label(out, s, p);
                }
            }
            (Kind::Build, Start) => {
                let name = t(50);
                title(out, name, Self::place_title_width(name, font), at::TITLE_Y);
                for (s, p) in [(t(144), (0.325, at::ROW_LOW)), (t(150), (0.605, at::ROW_HIGH)), (t(176), (0.605, at::ROW_LOW)), (t(live::BACK), (0.89, at::ROW_LOW))] {
                    place_label(out, s, p);
                }
            }
            (Kind::Build, Preview) => {
                let price = format!("≈ $ {}", self.shown(game).map_or(0, |s| s.price));
                label(out, &price, at::BUILD_PRICE.0, at::BUILD_PRICE.1, Self::label(font, &price).0, HALF_SMALL);
                let name = t(39);
                title(out, name, Self::place_title_width(name, font), at::TITLE_Y);
                for (s, p) in [(t(live::BUILD), (0.325, at::ROW_LOW)), (t(112), (0.605, at::ROW_HIGH)), (t(176), (0.605, at::ROW_LOW)), (t(live::BACK), (0.89, at::ROW_LOW))] {
                    place_label(out, s, p);
                }
            }
            (Kind::Buy, Between) => section_title(out),
            _ => section_title(out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt::ui::keys::{A, B};
    use crate::catalog::{Catalog, Item};

    fn font() -> Font {
        Font { name: "f".into(), latin: 1, size: 12, line: 37, max_code: 0x7f, map: Vec::new(), pages: Vec::new(), glyphs: Vec::new(), tail: [0; 6] }
    }

    fn game() -> Game {
        let item = Item { guid: 0x7504_1efb, name: "Chair".into(), description: "A chair.".into(), price: 79, effects: vec![(6, 2)] };
        Game { catalog: Catalog { buy: vec![vec![item]] }, ..Game::default() }
    }

    fn move_of(p: &(u32, Press)) -> Move {
        match p.1 {
            Press::Shop(m) | Press::ShopNow(m) => m,
            _ => panic!("not a move"),
        }
    }

    #[test]
    fn a_press_brings_the_next_screen_after_the_games_delay() {
        let mut s = Shop::new(Kind::Buy);
        let out = s.key(A, 2019);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].0, move_of(&out[0])), (2027, Move { from: Screen::Sections, to: Screen::Items }));
        // (a press while a change is on its way is not taken)
        assert!(s.key(A, 2020).is_empty());
        // build's list opens at once; its place mode comes 8 frames after the press and the list
        // drops its item as the press comes
        let mut b = Shop::new(Kind::Build);
        assert_eq!(b.key(A, 2683)[0].0, 2683);
        b.apply(Move { from: Screen::Sections, to: Screen::Items }, 2683, &game(), &font(), (0.0, 0.0));
        let out = b.key(A, 2766);
        assert_eq!(out.iter().map(|p| p.0).collect::<Vec<_>>(), vec![2766, 2774]);
    }

    #[test]
    fn only_the_first_screen_leaves_for_the_main_row() {
        let mut s = Shop::new(Kind::Buy);
        assert!(s.leaves());
        s.apply(Move { from: Screen::Sections, to: Screen::Items }, 10, &game(), &font(), (0.0, 0.0));
        assert!(!s.leaves());
        assert_eq!(s.key(B, 100)[0].0, 108);
    }

    #[test]
    fn place_mode_holds_the_price_and_placing_hands_the_object_over() {
        let g = game();
        let mut s = Shop::new(Kind::Buy);
        s.apply(Move { from: Screen::Sections, to: Screen::Items }, 10, &g, &font(), (0.0, 0.0));
        s.apply(Move { from: Screen::Items, to: Screen::Place }, 20, &g, &font(), (0.0, 0.0));
        assert_eq!(s.take_events(), vec![Event::Reserve(79)]);
        // placed on the cursor's tile (29.37, 8.57), its middle
        s.apply(Move { from: Screen::Place, to: Screen::Placed }, 30, &g, &font(), (29.37, 8.57));
        assert_eq!(s.take_events(), vec![Event::Purchase(Purchase { guid: 0x7504_1efb, price: 79, at: (472, 136), level: 1, facing: 0 })]);
        // B puts the list back after a pause of its own
        let (_, follow) = s.apply(Move { from: Screen::Placed, to: Screen::Between }, 40, &g, &font(), (0.0, 0.0));
        assert_eq!(follow.len(), 1);
        assert_eq!(follow[0].0, 48);
    }

    #[test]
    fn the_info_window_asks_for_the_items_motives() {
        let g = game();
        let mut s = Shop::new(Kind::Buy);
        s.apply(Move { from: Screen::Sections, to: Screen::Items }, 10, &g, &font(), (0.0, 0.0));
        let a = s.answers(&g, &font());
        assert!(a.contains(&("numberOfInfoLozenges".to_string(), "1".to_string())));
        assert!(a.contains(&("infoWindowMotive1".to_string(), "6".to_string())));
    }
}
