//! The pause screen's sections as the game runs them around the movies (`docs/assets.md`, "The
//! pause screen's sections"): which screen has the pad, what the game does when a movie sends
//! `draw_goals`, `draw_main` and the other commands of the pause screen (the calls it makes on
//! the wrapper), the answers it gives the movies' reads and the lines of text it draws.
//!
//! The game reads the pad itself. A press reaches the movie holding the focus two game frames
//! after the pad changed (the options screen reads it in the frame itself); the movie answers
//! with commands, and the game's handler for each runs as the command arrives: it calls the
//! wrapper (`destroyButton`, `DPadUp`, `showDialog`, ...), which asks for the section's movie to
//! be loaded.
//!
//! This is the presentation's side: it reads the world through a `PlayerView` and keeps only the
//! screen's own state; nothing here changes the world.

use std::collections::VecDeque;

use super::font::Font;
use crate::hud::{Call, PlayerView, SCREEN, Text, line};
use crate::quickdata::Image;

/// The sections of the pause screen's icon row, in the row's order (the movie's
/// `current_main_menu` is the index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Goals,
    Buy,
    Build,
    Careers,
    Options,
    Save,
    Quit,
}

impl Section {
    pub const ALL: [Section; 7] = [Section::Goals, Section::Buy, Section::Build, Section::Careers, Section::Options, Section::Save, Section::Quit];

    /// The section a `draw_X` command names (`draw_career`, not `draw_careers`).
    fn of_command(name: &str) -> Option<Section> {
        Some(match name {
            "draw_goals" => Section::Goals,
            "draw_buy" => Section::Buy,
            "draw_build" => Section::Build,
            "draw_career" => Section::Careers,
            "draw_options" => Section::Options,
            "draw_save" => Section::Save,
            "draw_quit" => Section::Quit,
            _ => return None,
        })
    }
}

/// The strings of the game's interface the pause screen draws: the language image's string
/// tables (`LiveModeMenuUIStrings`, `GlobalStrings`) by the index of the string in its table (the
/// tables are in the order of the game's keys, which are not on the disc), the houses' names and
/// goals (`HouseData`, its goal records) and the careers' names.
#[derive(Clone, Debug, Default)]
pub struct GameText {
    live: Vec<String>,
    global: Vec<String>,
    houses: Vec<HouseText>,
    careers: Vec<(u32, String)>,
}

/// One house of `HouseData`: its name and what its goals say.
#[derive(Clone, Debug, Default)]
struct HouseText {
    name: String,
    goals: Vec<GoalText>,
}

/// A goal's text (tokens in angle brackets still in it) and whether the goal's definition keeps
/// it out of the list until it is met (the definition's third word is not 0).
#[derive(Clone, Debug, Default)]
struct GoalText {
    text: String,
    hidden: bool,
}

/// Positions in `LiveModeMenuUIStrings` of the strings the pause screen draws.
mod live {
    pub const ALL_CAREERS: usize = 25;
    pub const AUTO_CENTER: usize = 28;
    pub const BACK: usize = 29;
    pub const BUILD: usize = 35;
    pub const BUY: usize = 45;
    pub const CAREERS: usize = 48;
    pub const FAST_FORWARD_TOGGLE: usize = 65;
    pub const FREE_WILL: usize = 70;
    pub const GOALS: usize = 76;
    pub const LOCKED: usize = 84;
    pub const MUSIC_VOLUME: usize = 90;
    pub const OFF: usize = 97;
    pub const ON: usize = 98;
    pub const OPTIONS: usize = 99;
    pub const QUIT: usize = 117;
    pub const QUIT_WARNING: usize = 120;
    pub const RUMBLE_FEATURE: usize = 127;
    pub const SAVE: usize = 130;
    pub const SAVE_AND_QUIT: usize = 131;
    pub const SAVE_QUESTION: usize = 132;
    pub const SELECT: usize = 139;
    pub const JUST_QUIT: usize = 94;
    pub const SOUND_EFFECTS_VOLUME: usize = 143;
    pub const THE: usize = 156;
    pub const BUTTON: usize = 42;
    pub const UNLOCKED: usize = 172;
    pub const WALLS: usize = 175;
    pub const SEATING: usize = 138;
}

/// Positions in `GlobalStrings`.
mod global {
    pub const CANCEL: usize = 14;
    pub const NO: usize = 55;
    pub const YES: usize = 95;
}

/// The position of "ACCEPT" in `CreateASimStrings` (the options screen's button).
const CREATE_ACCEPT: usize = 0;

/// The careers the pause screen lists: the Bustin' Out careers, ids 17 to 23 in `SimsCareers`.
const CAREER_IDS: std::ops::RangeInclusive<u32> = 17..=23;

impl GameText {
    /// Reads the tables from `quickdat.arc`'s bytes (the first language's images, English).
    pub fn read(quick: &[u8]) -> Option<GameText> {
        let entries = crate::arc::entries(quick)?;
        let image = |name: &str| entries.iter().find(|e| e.name == name).and_then(|e| Image::read_language(crate::arc::bytes(quick, e), 1));
        let ui = image("SimsUI")?;
        let mut live = ui.strings("LiveModeMenuUIStrings");
        let global = ui.strings("GlobalStrings");
        // "ACCEPT" is in another table: kept after the others so the positions above hold
        live.push(ui.strings("CreateASimStrings").get(CREATE_ACCEPT).cloned().unwrap_or_default());
        let houses = ui
            .rows("HouseData")
            .map(|r| {
                let goals = ui
                    .ptr(r + 32)
                    .map(|list| {
                        let n = list.checked_sub(4).and_then(|c| ui.u32(c)).unwrap_or(0) as usize;
                        (0..n)
                            .map(|k| {
                                let g = list + 12 * k;
                                let hidden = ui.ptr(g + 8).and_then(|d| ui.u32(d + 8)).is_some_and(|w| w != 0);
                                GoalText { text: ui.text(g).unwrap_or_default(), hidden }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                HouseText { name: ui.text(r).unwrap_or_default(), goals }
            })
            .collect();
        let careers = image("SimsCareers")
            .map(|c| {
                let mut careers: Vec<(u32, String)> = c.rows("Career").filter_map(|r| Some((c.u32(r)?, c.text(r + 8)?))).filter(|(id, _)| CAREER_IDS.contains(id)).collect();
                careers.sort_by_key(|c| c.0);
                careers
            })
            .unwrap_or_default();
        Some(GameText { live, global, houses, careers })
    }

    fn live(&self, k: usize) -> &str {
        self.live.get(k).map_or("", String::as_str)
    }

    fn global(&self, k: usize) -> &str {
        self.global.get(k).map_or("", String::as_str)
    }

    fn accept(&self) -> &str {
        self.live.last().map_or("", String::as_str)
    }

    /// The names of the pause screen's icons, in the row's order.
    fn icon(&self, section: Section) -> &str {
        self.live(match section {
            Section::Goals => live::GOALS,
            Section::Buy => live::BUY,
            Section::Build => live::BUILD,
            Section::Careers => live::CAREERS,
            Section::Options => live::OPTIONS,
            Section::Save => live::SAVE,
            Section::Quit => live::QUIT,
        })
    }

    /// A goal's text with its tokens filled: `<The1>` is "the ", `<R1>` the controller's R
    /// button and `<Button1>` " Button" (the tokens name the controller's buttons and the words
    /// round them; GameCube's here).
    fn goal_text(&self, text: &str) -> String {
        text.replace("<The1>", self.live(live::THE)).replace("<R1>", "R").replace("<Button1>", self.live(live::BUTTON))
    }
}

/// What the game gives the pause screen besides the world: the strings and the family's funds.
#[derive(Clone, Debug, Default)]
pub struct Game {
    pub text: GameText,
    /// The family's funds as the save keeps them (the pause screen's second funds line, drawn
    /// at the clock's place; the household's worth).
    pub family_funds: i32,
}

/// The game's options (the options screen's six rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub free_will: bool,
    pub rumble: bool,
    pub auto_center: bool,
    pub fast_forward_toggle: bool,
    pub sound_volume: u8,
    pub music_volume: u8,
}

impl Default for Settings {
    /// The game's defaults, as the options screen shows them on a new save.
    fn default() -> Settings {
        Settings { free_will: true, rumble: false, auto_center: true, fast_forward_toggle: false, sound_volume: 10, music_volume: 10 }
    }
}

const VOLUME_MAX: u8 = 10;
const OPTION_ROWS: usize = 6;

impl Settings {
    /// The row's label's position in `LiveModeMenuUIStrings` and its value as text.
    fn row(&self, k: usize, text: &GameText) -> (String, String) {
        let on = |b: bool| text.live(if b { live::ON } else { live::OFF }).to_string();
        let (label, value) = match k {
            0 => (live::FREE_WILL, on(self.free_will)),
            1 => (live::RUMBLE_FEATURE, on(self.rumble)),
            2 => (live::AUTO_CENTER, on(self.auto_center)),
            3 => (live::FAST_FORWARD_TOGGLE, on(self.fast_forward_toggle)),
            4 => (live::SOUND_EFFECTS_VOLUME, self.sound_volume.to_string()),
            _ => (live::MUSIC_VOLUME, self.music_volume.to_string()),
        };
        (text.live(label).to_string(), value)
    }

    /// The row's value changes by a step left (-1) or right (1).
    fn step(&mut self, k: usize, d: i8) {
        let volume = |v: &mut u8| *v = (i16::from(*v) + i16::from(d)).clamp(0, i16::from(VOLUME_MAX)) as u8;
        match k {
            0 => self.free_will ^= true,
            1 => self.rumble ^= true,
            2 => self.auto_center ^= true,
            3 => self.fast_forward_toggle ^= true,
            4 => volume(&mut self.sound_volume),
            _ => volume(&mut self.music_volume),
        }
    }
}

// ---- what the log measured of the game's layout and text (pause-c.log) ----------------------

/// Half the height of a line's glyph box as a fraction of the screen, for the two sizes the
/// pause screen draws at: a line is placed with its middle on a y the movies or the layout give,
/// its top is that y less this. Measured: a face record's y against its aligned y (the buttons'
/// 0.905: 0.88582385; the selected option's 0.268527: 0.24661142).
const HALF_SMALL: f32 = 0.0191761;
const HALF_BIG: f32 = 0.0219156;

/// The width the wrapper gives the buttons of the options screen, the width of a goal's status
/// label and of a dialog's button (the reads `UI_button_width`, `goal_status_width` and the
/// `drawButton` of the options screen's two buttons, 0.103551: the width of "ACCEPT" and of
/// "CANCEL", which have the same glyph columns).
const BUTTON_WIDTH: f32 = 0.103551;

/// What the dialog's title bar reads (`dialog_title_bar_width`, frame 2599), the dialog's box
/// (`dialog_x`, `dialog_box_width`, `dialog_box_height`) and the quit dialog's 3-button row.
const DIALOG_TITLE_WIDTH: f32 = 0.117188;
const DIALOG_BOX: (f32, f32, f32, f32) = (0.15, 0.7, 0.55, 0.18);

/// The options screen's box (reads of frame 2457).
const OPTIONS_BOX: (f32, f32) = (0.75, 0.6);

/// The buy and build screens' counts, which the catalog gives (a later packet): the sections
/// and the items of the first (reads of frames 2028 to 2029 and 2171 to 2172).
const BUY_COUNTS: (u32, u32) = (9, 46);
const BUILD_COUNTS: (u32, u32) = (8, 4);

/// The goals list: the box's width (the read `goal_item_width`, 0.423153).
const GOAL_ITEM_WIDTH: f32 = 0.423153;
/// Where the lines are broken, in glyph columns (a fit: the log's breaks hold for any limit
/// from 445 to 553 for a selected goal's text, 419 to 440 for the others' and 536 to 553 for the
/// quit dialog's warning; the columns of the lines that fit and of the first that does not).
const GOAL_WRAP_BIG: f32 = 480.0;
const GOAL_WRAP_SMALL: f32 = 430.0;
const DIALOG_WRAP: f32 = 545.0;
/// Where the goals list's lines are centred (the selected goal's size words' aligned x).
const GOAL_CENTRE: f32 = 0.33;
/// The selected goal's first line's middle y, its lines' pitch and the next goals' rows (the
/// log's lines: 0.297 and 0.333; the plain goal rows' tops 0.3925633, 0.5025634, 0.6125634 with
/// a pitch of 0.032 between their lines).
const GOAL_SELECTED_Y: f32 = 0.297;
const GOAL_SELECTED_PITCH: f32 = 0.036;
const GOAL_ROW_TOP: f32 = 0.3925633;
const GOAL_ROW_PITCH: f32 = 0.11;
const GOAL_LINE_PITCH: f32 = 0.032;
/// The rows the list shows.
const GOAL_ROWS: usize = 4;
/// The goals' text is drawn this many game frames after `p_goals_ready` (1887 to 1891).
const GOALS_TEXT_DELAY: u32 = 4;
/// The goals screen's own lines: the title (centred on 0.48), the house's name (0.51) and the
/// unlock label (0.80348957) with their middle ys.
const GOALS_TITLE: (f32, f32) = (0.48, 0.055084415);
const GOALS_HOUSE: (f32, f32) = (0.51, 0.175);
const GOALS_STATUS: (f32, f32) = (0.80348957, 0.67);

/// The careers screen: the title (centred on 0.48, top 0.058084413), the "all careers" line and
/// the career lines' centre, first middle y and pitch.
const CAREERS_TITLE: (f32, f32) = (0.48, 0.058084413);
const CAREERS_ALL: (f32, f32) = (0.48, 0.18);
const CAREERS_CENTRE: f32 = 0.42656252;
const CAREERS_FIRST_Y: f32 = 0.278;
const CAREERS_PITCH: f32 = 0.0535;

/// The options rows' values drawn at the volumes' rows are this much lower than their labels.
const VOLUME_DROP: f32 = 0.008;

/// The dialog's text block: its first line's top, the pitch from the first line to the second
/// and the pitch after it (19.96 and 20 pixels), and half the glyph box of the quit dialog's
/// three buttons (6/7 of the small size's).
const DIALOG_TEXT_TOP: f32 = 0.30892858;
const DIALOG_FIRST_PITCH: f32 = 0.0445617;
const DIALOG_LINE_PITCH: f32 = 20.0 / 448.0;
const HALF_QUIT_BUTTON: f32 = HALF_SMALL * 6.0 / 7.0;

/// The title of the pause's main row: its top (the movie's title bar place is not reported).
const MAIN_TITLE_Y: f32 = 0.18608442;

/// The buy and build screens' title: its top; the build screen draws its buttons this many game
/// frames after its movie's load (2169 to 2179).
const SECTION_TITLE_Y: f32 = 0.05308442;
const BUILD_BUTTONS_DELAY: u32 = 10;

/// The main row's two buttons: where the game puts them (0x800A5018 and 0x800A504C) and the
/// back button of the sections that draw one.
const SELECT_AT: (f32, f32) = (0.4, 0.905);
const BACK_AT: (f32, f32) = (0.7, 0.905);
const BACK_CENTRED: (f32, f32) = (0.5, 0.905);
/// The options screen's buttons (0x800FB8A4 and 0x800FB914).
const ACCEPT_AT: (f32, f32) = (0.35, 0.9);
const CANCEL_AT: (f32, f32) = (0.65, 0.9);

/// The faces the pause screen draws its lines in: the big size (a selected row, a title line, a
/// button), the small one (the rows that are not selected) and the dialog's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Big,
    Small,
    Dialog,
}

/// A big line is this much wider than the same line small (ON: 28.4243 against 24.3712).
const BIG_OVER_SMALL: f32 = 7.0 / 6.0;

/// The widths of lines as the game measured them, in pixels, for the strings the pause screen
/// of Mom's house draws. The font's glyph columns do not give a width (the advance is not the
/// columns, `docs/assets.md`), so these come from the log (pause-c.log): a line with an aligned
/// point (size words) is twice its distance to its place; a plain line of a list that is
/// centred on a place the movie reports (the options' labels on `list_item1_x`, their values on
/// `arrows_x`) or on 0.33 (the goals' lines) is twice that distance. "10" is the width that
/// puts the volume values where the game does (they are not centred on `arrows_x`).
const MEASURED: &[(&str, Face, f32)] = &[
    ("ACCEPT", Face::Big, 66.2727),
    ("All Careers", Face::Big, 105.0909),
    ("BACK", Face::Big, 45.0303),
    ("CANCEL", Face::Big, 66.2727),
    ("Counter Culture:", Face::Big, 302.4243),
    ("FREE WILL", Face::Big, 96.2424),
    ("Fashion Victim:", Face::Big, 269.5758),
    ("Gangster:", Face::Big, 169.4545),
    ("Jock:", Face::Big, 96.7273),
    ("Mad Scientist:", Face::Big, 252.7273),
    ("Mom's House", Face::Big, 120.4243),
    ("Movie Star:", Face::Big, 204.8485),
    ("ON", Face::Big, 28.4243),
    ("OPTIONS", Face::Big, 72.2273),
    // (not a line the log drew big: the read `item_width`, 0.385038, is the widest label's)
    ("SOUND EFFECTS VOLUME", Face::Big, 246.4243),
    ("Paramilitary:", Face::Big, 220.8485),
    ("Press the R Button for double", Face::Big, 233.2727),
    ("SELECT", Face::Big, 59.8561),
    ("UNLOCKED", Face::Big, 89.2651),
    ("speed.", Face::Big, 49.356),
    ("10", Face::Small, 7.1312),
    ("AUTO CENTER", Face::Small, 116.8712),
    ("Do fun stuff to raise your", Face::Small, 165.1363),
    ("Eat enough food to fill your", Face::Small, 181.2727),
    ("FAST FORWARD TOGGLE", Face::Small, 205.75),
    ("MUSIC VOLUME", Face::Small, 128.0757),
    ("OFF", Face::Small, 31.4924),
    ("ON", Face::Small, 24.3712),
    ("RUMBLE FEATURE", Face::Small, 142.9015),
    ("SOUND EFFECTS VOLUME", Face::Small, 211.1212),
    ("Use a toilet to empty your", Face::Small, 171.1363),
    ("bladder", Face::Small, 48.4091),
    ("fun level", Face::Small, 56.1818),
    ("stomach", Face::Small, 51.9091),
    ("Are you sure you want to save?", Face::Dialog, 300.0),
    ("CANCEL", Face::Dialog, 55.0909),
    ("JUST QUIT", Face::Dialog, 72.6818),
    ("NO", Face::Dialog, 24.3712),
    ("SAVE AND QUIT", Face::Dialog, 109.5909),
    ("YES", Face::Dialog, 29.1591),
    ("since your last save.  Do you want", Face::Dialog, 338.4),
];

/// The titles' widths as the movies read them (`title_bar_width` and the like), fractions of
/// the screen, by the string. The same title is drawn that wide.
const TITLES: &[(&str, f32)] = &[
    ("GOALS", 0.102652),
    ("BUY", 0.056534),
    ("BUILD", 0.081818),
    ("CAREERS", 0.133144),
    ("OPTIONS", 0.132102),
    ("SAVE", 0.078030),
    ("QUIT", 0.071780),
    ("Seating/Beds", 0.180208),
    // the buttons' labels, as the game gave `drawButton` their width
    ("SELECT", 0.093525),
    ("BACK", 0.070360),
];

/// The glyph columns of a line, a space `SPACE_COLUMNS` (the font has no glyph for it).
fn columns(font: &Font, text: &str) -> f32 {
    text.chars().map(|c| font.glyph(c as u32).map_or(SPACE_COLUMNS, |g| (g.x1 - g.x0) as f32)).sum()
}

/// The advance of a space in texels (`hud::SPACE`'s fit: the widths of the logged lines give 10.6
/// to 12).
const SPACE_COLUMNS: f32 = 11.0;

/// What a line's width is when the log has not measured it: the glyph columns at a scale per
/// face. Rough (the advance is not the columns: 3 to 17 pixels off the measured lines, a fit
/// of them); only strings the log never drew come to it.
fn approx_width(font: &Font, text: &str, face: Face) -> f32 {
    let scale = match face {
        Face::Big => 0.64,
        Face::Small => 0.55,
        Face::Dialog => 0.6,
    };
    columns(font, text) * scale
}

/// A line's width in pixels in a face.
fn width(font: &Font, text: &str, face: Face) -> f32 {
    let get = |f: Face| MEASURED.iter().find(|m| m.0 == text && m.1 == f).map(|m| m.2);
    get(face)
        .or_else(|| match face {
            Face::Big => get(Face::Small).map(|w| w * BIG_OVER_SMALL),
            Face::Small => get(Face::Big).map(|w| w / BIG_OVER_SMALL),
            Face::Dialog => None,
        })
        .unwrap_or_else(|| approx_width(font, text, face))
}

/// A title's width as a fraction of the screen.
fn title_width(font: &Font, text: &str) -> f32 {
    TITLES.iter().find(|t| t.0 == text).map_or_else(|| width(font, text, Face::Big) / SCREEN.0, |t| t.1)
}

/// The calls the game makes; a helper for a call with strings.
fn make(name: &'static str, args: &[&str]) -> Call {
    Call { name, args: args.iter().map(|s| s.to_string()).collect() }
}

fn dpad(up: &str, down: &str, left: &str, right: &str) -> Vec<Call> {
    vec![make("DPadUp", &[up]), make("DPadDown", &[down]), make("DPadLeft", &[left]), make("DPadRight", &[right])]
}

fn destroy(button: &'static str) -> Call {
    make("destroyButton", &[button])
}

fn draw_button(name: &'static str, at: (f32, f32), w: f32) -> Call {
    Call { name: "drawButton", args: vec![name.into(), format!("{:.6}", at.0), format!("{:.6}", at.1), format!("{:.6}", w)] }
}

/// What a press does once the game has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// The key goes to the movie that has the focus.
    Key(u32),
    /// B in a section (but the options): the game itself takes the main row back (none of the
    /// movies has a `keyPress_circle`).
    Back,
    /// The options screen (the game's own, reading the pad in the frame): A takes the options as
    /// they stand, B puts them back; either leaves.
    OptionsLeave { keep: bool },
}

/// Where the pad's presses go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Main,
    Section(Section),
}

impl Focus {
    fn dialog(self) -> bool {
        matches!(self, Focus::Section(Section::Save | Section::Quit))
    }

    fn filter_dialog(self) -> Focus {
        if self.dialog() { self } else { Focus::Main }
    }
}

/// The pause screen's state while it is up.
#[derive(Clone, Debug)]
pub struct Pause {
    focus: Focus,
    /// The icon the main row is on (the last `current_main_menu`).
    current: Section,
    /// Presses waiting for their frame: (frame, press).
    waiting: VecDeque<(u32, Press)>,
    /// The main row has been up (`pause_starting`): the funds lines are drawn from then on.
    started: bool,
    /// The main row's movie shows (`draw_main`), has said which icon it is on (`current_main_menu`
    /// since then) and its buttons are up (`pause_button_*_ready`, at a place on the screen).
    main_shown: bool,
    title_known: bool,
    buttons: bool,
    accept_x: f32,
    /// The frame the section's movie was loaded at and when it said it was ready.
    opened_at: u32,
    ready_at: Option<u32>,
    /// The options screen's buttons are up (`option_button_*_ready`).
    option_buttons: bool,
    /// The dialog's movie says it is up.
    dialog_ready: bool,
    /// The goals list's selected goal.
    goal: usize,
    /// The options' selected row, the options as they stand and as they were when the screen
    /// opened.
    item: usize,
    settings: Settings,
    kept: Settings,
    /// Places the sections' movies reported (fractions of the screen): the options' two list
    /// rows and arrows, the quit dialog's three buttons.
    list: [(f32, f32); 2],
    arrows_x: f32,
    quit_buttons: [(f32, f32); 3],
    /// The title bar width the movie last read, for a title that is placed before its own
    /// width is read (the build screen's).
    title_width: f32,
}

/// The game's frames from a pad change to the movie's key (the options screen reads the pad
/// itself, in the same frame). Measured: the pad changes of frames 1882, 1969, 2002 and the
/// commands they bring at 1884, 1971, 2004.
const KEY_DELAY: u32 = 2;

/// The game's frames from B on the buy or build screen to its calls back to the main row
/// (pad 2112 and 2255, calls 2120 and 2263).
const CATALOG_BACK_DELAY: u32 = 8;

impl Pause {
    pub fn new(settings: Settings) -> Pause {
        Pause {
            focus: Focus::Main,
            current: Section::Goals,
            waiting: VecDeque::new(),
            started: false,
            main_shown: false,
            title_known: false,
            buttons: false,
            accept_x: -1.0,
            opened_at: 0,
            ready_at: None,
            option_buttons: false,
            dialog_ready: false,
            goal: 0,
            item: 0,
            settings,
            kept: settings,
            list: [(0.0, 0.0); 2],
            arrows_x: 0.0,
            quit_buttons: [(0.0, 0.0); 3],
            title_width: 0.0,
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// The pad's pressed buttons of one game frame (`frame`): the game reads them and acts on
    /// them two frames later (at once on the options screen).
    pub fn pad(&mut self, pressed: u16, frame: u32) {
        use super::ui::{keys, pad};
        let options = self.focus == Focus::Section(Section::Options);
        if pressed & pad::START != 0 && self.focus == Focus::Main {
            // Start on the main row is the triangle's: `p_main` answers with `pauseDone`
            self.waiting.push_back((frame + KEY_DELAY, Press::Key(keys::TRIANGLE)));
        }
        for (bit, key) in [(pad::LEFT, keys::LEFT), (pad::RIGHT, keys::RIGHT), (pad::UP, keys::UP), (pad::DOWN, keys::DOWN), (pad::A, keys::A), (pad::B, keys::B)] {
            if pressed & bit == 0 {
                continue;
            }
            let press = match (self.focus, key) {
                // (B is the triangle's key to the movies: `p_main` answers with `pauseDone`)
                (Focus::Main, keys::B) => Press::Key(keys::TRIANGLE),
                (Focus::Section(Section::Options), keys::B) => Press::OptionsLeave { keep: false },
                (Focus::Section(_), keys::B) => Press::Back,
                (Focus::Section(Section::Options), keys::A) => Press::OptionsLeave { keep: true },
                _ => Press::Key(key),
            };
            let delay = match (self.focus, press) {
                (Focus::Section(Section::Buy | Section::Build), Press::Back) => CATALOG_BACK_DELAY,
                _ => KEY_DELAY,
            };
            self.waiting.push_back((if options { frame } else { frame + delay }, press));
        }
    }

    /// The presses that are due at this frame; the settings follow the options' keys.
    pub fn due(&mut self, frame: u32) -> Vec<Press> {
        let mut out = Vec::new();
        while self.waiting.front().is_some_and(|w| w.0 <= frame) {
            let (_, press) = self.waiting.pop_front().unwrap_or((0, Press::Back));
            if self.focus == Focus::Section(Section::Options)
                && let Press::Key(k) = press
            {
                use super::ui::keys;
                match k {
                    keys::UP => self.item = self.item.saturating_sub(1),
                    keys::DOWN => self.item = (self.item + 1).min(OPTION_ROWS - 1),
                    keys::LEFT => self.settings.step(self.item, -1),
                    keys::RIGHT => self.settings.step(self.item, 1),
                    _ => {}
                }
            }
            if self.focus == Focus::Section(Section::Goals)
                && let Press::Key(k) = press
            {
                use super::ui::keys;
                match k {
                    keys::UP => self.goal = self.goal.saturating_sub(1),
                    keys::DOWN => self.goal += 1,
                    _ => {}
                }
            }
            out.push(press);
        }
        out
    }

    /// Whether the section the game is on is a dialog (B is the triangle key to its movie, which
    /// answers with `dialog_key_press`).
    pub fn dialog(&self) -> bool {
        self.focus.dialog()
    }

    /// Whether the calls of leaving the section the game is on come after the frame's loads (the
    /// buy and build screens': 2120 calls, 2121 loads p_main).
    pub fn late_back(&self) -> bool {
        matches!(self.focus, Focus::Section(Section::Buy | Section::Build))
    }

    /// A section is left with B: the calls the game makes (0x800A4C24 to 0x800A51A8; they bring
    /// the movies' `draw_main`).
    pub fn back(&mut self) -> Vec<Call> {
        match self.focus {
            Focus::Section(s) if !matches!(s, Section::Save | Section::Quit) => Self::close_calls(s),
            _ => Vec::new(),
        }
    }

    /// The options screen is left with a press: the calls it makes (0x800F97C4 to 0x800F9840,
    /// then the pause screen's own back to the main row).
    pub fn leave_options(&mut self, keep: bool) -> Vec<Call> {
        if !keep {
            self.settings = self.kept;
        }
        self.kept = self.settings;
        self.focus = Focus::Main;
        self.option_buttons = false;
        // the pause movies load again: the main row starts over
        self.started = false;
        self.main_shown = false;
        self.buttons = false;
        let mut calls = vec![make("hideOption", &[]), destroy("accept"), destroy("decline"), make("resetButtonContext", &[]), make("showPause", &[]), make("showMainPause", &[])];
        calls.extend(dpad("0", "0", "1", "1"));
        calls
    }

    /// The game's handler of a command of a pause screen's movie (`Screen_Command`, the
    /// screens' vtable slot 0x44): what it makes of it and the calls it makes at once.
    pub fn command(&mut self, name: &str, argument: &str, frame: u32, text: &GameText, font: &Font) -> Vec<Call> {
        let num = argument.parse::<f32>().unwrap_or(0.0);
        let mut calls = Vec::new();
        match name {
            "current_main_menu" => {
                if let Some(s) = argument.parse::<usize>().ok().and_then(|k| Section::ALL.get(k)) {
                    self.current = *s;
                    self.title_known = true;
                    self.title_width = title_width(font, text.icon(*s));
                }
            }
            "pause_starting" if argument == "1" => self.started = true,
            "p_main_ready" if argument == "1" => {
                calls.push(draw_button("accept", SELECT_AT, title_width(font, text.live(live::SELECT))));
                calls.push(draw_button("decline", BACK_AT, title_width(font, text.live(live::BACK))));
            }
            "pause_button_accept_x" => {
                self.accept_x = num;
                self.buttons &= num >= 0.0;
            }
            "pause_button_accept_ready" => self.buttons = argument == "true" && self.accept_x >= 0.0,
            "draw_main" => {
                self.main_shown = argument == "true";
                self.title_known = false;
                if self.main_shown
                    && let Focus::Section(s) = self.focus
                    && s != Section::Options
                {
                    self.focus = Focus::Main;
                    self.dialog_ready = false;
                }
            }
            "options_ready" | "dialog_ready" | "p_goals_ready" | "p_career_ready" | "p_buy_ready" | "p_build_ready" => {
                if name == "dialog_ready" {
                    self.dialog_ready = argument == "1";
                } else {
                    self.ready_at = Some(frame);
                }
            }
            "done_display" if argument == "done" && self.focus == Focus::Section(Section::Options) => {
                calls.push(draw_button("accept", ACCEPT_AT, BUTTON_WIDTH));
                calls.push(draw_button("decline", CANCEL_AT, BUTTON_WIDTH));
            }
            "option_button_accept_ready" => self.option_buttons = argument == "true",
            "option_button_accept_x" if num < 0.0 => self.option_buttons = false,
            "current_item" => self.item = argument.parse().unwrap_or(0),
            // (a dialog's cancel: the game takes the main row back at once)
            "dialog_key_press" if argument == "cancel" => {
                if let Focus::Section(s) = self.focus.filter_dialog() {
                    calls.extend(Self::close_calls(s));
                }
            }
            "list_item1_x" => self.list[0].0 = num,
            "list_item1_y" => self.list[0].1 = num,
            "list_item2_x" => self.list[1].0 = num,
            "list_item2_y" => self.list[1].1 = num,
            "arrows_x" => self.arrows_x = num,
            "bt_1_x" => self.quit_buttons[0].0 = num,
            "bt_1_y" => self.quit_buttons[0].1 = num,
            "bt_2_x" => self.quit_buttons[1].0 = num,
            "bt_2_y" => self.quit_buttons[1].1 = num,
            "bt_3_x" => self.quit_buttons[2].0 = num,
            "bt_3_y" => self.quit_buttons[2].1 = num,
            other => {
                if let Some(s) = Section::of_command(other).filter(|_| argument == "true" && self.focus == Focus::Main) {
                    self.focus = Focus::Section(s);
                    self.opened_at = frame;
                    self.ready_at = None;
                    self.goal = 0;
                    self.item = 0;
                    self.arrows_x = 0.0;
                    self.kept = self.settings;
                    calls.extend(Self::open_calls(s, font, text));
                }
            }
        }
        calls
    }

    /// What the game does when a section's movie is asked for.
    fn open_calls(s: Section, font: &Font, text: &GameText) -> Vec<Call> {
        // (the game passes the centre as the string ".5")
        let back = || Call { name: "drawButton", args: vec!["decline".into(), ".5".into(), format!("{:.6}", BACK_CENTRED.1), format!("{:.6}", title_width(font, text.live(live::BACK)))] };
        match s {
            Section::Goals => [vec![destroy("accept"), destroy("decline")], dpad("1", "1", "0", "0"), vec![back()]].concat(),
            Section::Careers => [vec![destroy("accept"), destroy("decline")], dpad("0", "0", "0", "0"), vec![back()]].concat(),
            Section::Buy | Section::Build => dpad("1", "1", "0", "0"),
            Section::Options => vec![destroy("accept"), destroy("decline"), make("showOption", &[]), make("setButtonContext", &["option"])],
            Section::Save => [dpad("0", "0", "0", "0"), vec![destroy("accept"), destroy("decline"), make("showDialog", &[])]].concat(),
            Section::Quit => [dpad("0", "0", "1", "1"), vec![destroy("accept"), destroy("decline"), make("showDialog", &[])]].concat(),
        }
    }

    /// What the game does to take the main row back (0x800A4C24 and its neighbours).
    fn close_calls(s: Section) -> Vec<Call> {
        let back = || [vec![make("showMainPause", &[])], dpad("0", "0", "1", "1")].concat();
        match s {
            Section::Goals | Section::Careers => [back(), vec![destroy("decline")]].concat(),
            Section::Options => Vec::new(),
            Section::Buy | Section::Build => [vec![make("showMainPause", &[]), make("showMainPause", &[])], dpad("0", "0", "1", "1"), dpad("0", "0", "1", "1")].concat(),
            Section::Save | Section::Quit => [vec![make("hideDialog", &[]), make("showMainPause", &[]), make("showMainPause", &[])], dpad("0", "0", "1", "1"), dpad("0", "0", "1", "1")].concat(),
        }
    }

    /// The answers to the reads of the movie that has the focus, by name.
    pub fn answers(&self, view: &PlayerView, game: &Game, font: &Font) -> Vec<(String, String)> {
        let f = |v: f32| format!("{v:.6}");
        let mut out: Vec<(String, String)> = vec![("pause_mode".into(), "resident".into()), ("pause_enable_buy_build".into(), "true".into()), ("pause_enable_only_quit".into(), String::new())];
        let text = &game.text;
        let mut put = |k: &str, v: String| out.push((k.to_string(), v));
        match self.focus {
            Focus::Main => put("title_bar_width", f(title_width(font, text.icon(self.current)))),
            Focus::Section(Section::Goals) => {
                let shown = self.shown_goals(view, text);
                put("goal_number_of_goals", shown.len().to_string());
                put("goal_current_goal", self.goal.to_string());
                put("goal_current_item", "0".into());
                put("goal_item_width", f(GOAL_ITEM_WIDTH));
                put("goal_level_title_bar_width", f(width(font, &self.house_name(view, text), Face::Big) / SCREEN.0));
                put("goal_status_width", f(BUTTON_WIDTH));
                put("goal_title_bar_width", f(title_width(font, text.icon(Section::Goals))));
            }
            Focus::Section(Section::Careers) => {
                for k in 0..CAREER_IDS.count() {
                    put(&format!("career_value_{k}"), view.career_levels[k].to_string());
                }
                put("career_dialog_title_width", f(width(font, text.live(live::ALL_CAREERS), Face::Big) / SCREEN.0));
                put("career_title_bar_width", f(title_width(font, text.icon(Section::Careers))));
            }
            Focus::Section(Section::Options) => {
                put("dialog_title_width", f(width(font, text.live(live::OPTIONS), Face::Big) / SCREEN.0));
                put("dialog_width", f(OPTIONS_BOX.0));
                put("dialog_height", f(OPTIONS_BOX.1));
                put("is_adjust_screen", "0".into());
                put("has_adjust_screen", "0".into());
                put("number_item", OPTION_ROWS.to_string());
                let widest = (0..OPTION_ROWS).map(|k| width(font, &self.settings.row(k, text).0, Face::Big)).fold(0.0, f32::max);
                put("item_width", f(widest / SCREEN.0));
            }
            Focus::Section(Section::Buy) => {
                put("number_of_sections", BUY_COUNTS.0.to_string());
                put("current_section_max", String::new());
                put("max_items", BUY_COUNTS.1.to_string());
                put("title_bar_width", f(title_width(font, text.live(live::SEATING))));
            }
            Focus::Section(Section::Build) => {
                // (the title bar width is the main row's: the build screen reads it before its
                // own title is known)
                put("title_bar_width", f(title_width(font, text.icon(Section::Build))));
                put("current_item", "0".into());
                put("current_section_max", String::new());
                put("number_of_sections", BUILD_COUNTS.0.to_string());
                put("max_items", BUILD_COUNTS.1.to_string());
            }
            Focus::Section(s @ (Section::Save | Section::Quit)) => {
                put("UI_button_width", f(BUTTON_WIDTH));
                put("number_of_buttons", if s == Section::Save { "2" } else { "3" }.into());
                put("is_critical_dialog", if s == Section::Save { "0" } else { "1" }.into());
                put("dialog_x", f(DIALOG_BOX.0));
                put("dialog_y", f(DIALOG_BOX.3));
                put("dialog_box_width", f(DIALOG_BOX.1));
                put("dialog_box_height", f(DIALOG_BOX.2));
                put("dialog_title_bar_width", f(DIALOG_TITLE_WIDTH));
                put("button_width", f(width(font, text.live(live::SAVE_AND_QUIT), Face::Dialog) / SCREEN.0));
                put("button_height", String::new());
                put("UI_button_height", String::new());
            }
        }
        out
    }

    fn house_name(&self, view: &PlayerView, text: &GameText) -> String {
        text.houses.get(view.house).map(|h| h.name.clone()).unwrap_or_default()
    }

    /// The goals the list shows: the house's goals that are not hidden or that the player has
    /// met, in order, with their texts.
    fn shown_goals(&self, view: &PlayerView, text: &GameText) -> Vec<String> {
        let Some(h) = text.houses.get(view.house) else { return Vec::new() };
        h.goals.iter().enumerate().filter(|(i, g)| !g.hidden || view.goals_met.get(*i).copied().unwrap_or(false)).map(|(_, g)| text.goal_text(&g.text)).collect()
    }

    /// Whether the pause screen's funds lines are drawn.
    pub fn funds_shown(&self) -> bool {
        self.started && !matches!(self.focus, Focus::Section(Section::Options))
    }

    /// The pause screen's lines of text for the frame, in the order the game draws them (the
    /// funds lines are the caller's, after them).
    pub fn texts(&self, view: &PlayerView, game: &Game, font: &Font, hud: &crate::hud::Layout, frame: u32, out: &mut Vec<Text>) {
        let text = &game.text;
        let centred = |out: &mut Vec<Text>, s: &str, face: Face, half: f32, centre: f32, y2: f32, shadow: bool| {
            let w = width(font, s, face);
            let (x, y) = (centre - w / SCREEN.0 / 2.0, y2 - half);
            let h = Some(2.0 * half * SCREEN.1);
            if shadow {
                line(out, s, x, y, Some(w), h);
            } else {
                out.push(Text { text: s.to_string(), x, y, shadow: false, width: Some(w), height: h, plain: true });
            }
        };
        // a title: plain, centred on a place, its top given
        let title = |out: &mut Vec<Text>, s: &str, centre: f32, top: f32| {
            let w = title_width(font, s) * SCREEN.0;
            out.push(Text { text: s.to_string(), x: centre - w / SCREEN.0 / 2.0, y: top, shadow: false, width: Some(w), height: Some(2.0 * HALF_BIG * SCREEN.1), plain: true });
        };
        let button = |out: &mut Vec<Text>, s: &str, at: (f32, f32)| {
            let w = title_width(font, s) * SCREEN.0;
            let (x, y) = (at.0 - w / SCREEN.0 / 2.0, at.1 - HALF_SMALL);
            line(out, s, x, y, Some(w), Some(2.0 * HALF_SMALL * SCREEN.1));
        };
        match self.focus {
            Focus::Main => {
                if self.main_shown && self.title_known {
                    title(out, text.icon(self.current), 0.5, MAIN_TITLE_Y);
                }
                if self.buttons {
                    button(out, text.live(live::SELECT), SELECT_AT);
                    button(out, text.live(live::BACK), BACK_AT);
                }
            }
            Focus::Section(Section::Goals) => {
                if self.ready_at.is_some_and(|r| frame >= r + GOALS_TEXT_DELAY) {
                    self.goal_texts(view, text, font, out);
                    title(out, text.icon(Section::Goals), GOALS_TITLE.0, GOALS_TITLE.1);
                    centred(out, &self.house_name(view, text), Face::Big, HALF_BIG, GOALS_HOUSE.0, GOALS_HOUSE.1, true);
                    let status = if view.goals_met.get(self.goal).copied().unwrap_or(false) { live::UNLOCKED } else { live::LOCKED };
                    centred(out, text.live(status), Face::Big, HALF_SMALL, GOALS_STATUS.0, GOALS_STATUS.1, true);
                    button(out, text.live(live::BACK), BACK_CENTRED);
                }
            }
            Focus::Section(Section::Careers) => {
                title(out, text.icon(Section::Careers), CAREERS_TITLE.0, CAREERS_TITLE.1);
                centred(out, text.live(live::ALL_CAREERS), Face::Big, HALF_BIG, CAREERS_ALL.0, CAREERS_ALL.1, true);
                for (k, (_, name)) in text.careers.iter().enumerate() {
                    centred(out, &format!("{name}:"), Face::Big, HALF_BIG, CAREERS_CENTRE, CAREERS_FIRST_Y + k as f32 * CAREERS_PITCH, true);
                }
                if self.ready_at.is_some() {
                    button(out, text.live(live::BACK), BACK_CENTRED);
                }
            }
            Focus::Section(Section::Options) => {
                if self.ready_at.is_some() || self.arrows_x > 0.0 {
                    centred(out, text.live(live::OPTIONS), Face::Big, HALF_SMALL, hud.title.0, hud.title.1, true);
                    let pitch = self.list[1].1 - self.list[0].1;
                    for k in 0..OPTION_ROWS {
                        let (label, value) = self.settings.row(k, text);
                        let y2 = self.list[0].1 + k as f32 * pitch;
                        let (face, half, shadow) = if k == self.item { (Face::Big, HALF_BIG, true) } else { (Face::Small, HALF_SMALL, false) };
                        centred(out, &label, face, half, self.list[0].0, y2, shadow);
                        let drop = if k >= OPTION_ROWS - 2 { VOLUME_DROP } else { 0.0 };
                        centred(out, &value, face, half, self.arrows_x, y2 + drop, shadow);
                    }
                    if self.option_buttons {
                        button(out, text.accept(), ACCEPT_AT);
                        button(out, text.global(global::CANCEL), CANCEL_AT);
                    }
                }
            }
            Focus::Section(s @ (Section::Save | Section::Quit)) => {
                if self.dialog_ready {
                    let t = hud.title;
                    title(out, text.icon(s), t.0, t.1 - HALF_BIG);
                    self.dialog_lines(s, text, font, out);
                    let dialog_button = |out: &mut Vec<Text>, label: &str, at: (f32, f32), face: Face| centred(out, label, face, HALF_SMALL, at.0, at.1, true);
                    let quit_button = |out: &mut Vec<Text>, label: &str, at: (f32, f32)| centred(out, label, Face::Dialog, HALF_QUIT_BUTTON, at.0, at.1, true);
                    if s == Section::Quit {
                        for (label, at) in [(text.live(live::SAVE_AND_QUIT), self.quit_buttons[0]), (text.live(live::JUST_QUIT), self.quit_buttons[1]), (text.global(global::CANCEL), self.quit_buttons[2])] {
                            quit_button(out, label, at);
                        }
                    }
                    dialog_button(out, text.global(global::YES), hud.accept, Face::Dialog);
                    if s == Section::Save {
                        dialog_button(out, text.global(global::NO), hud.decline, Face::Dialog);
                    } else {
                        dialog_button(out, text.global(global::CANCEL), hud.decline, Face::Big);
                    }
                }
            }
            Focus::Section(s @ (Section::Buy | Section::Build)) => {
                let (name, w) = if s == Section::Buy { (text.live(live::SEATING), title_width(font, text.live(live::SEATING))) } else { (text.live(live::WALLS), self.title_width) };
                out.push(Text { text: name.to_string(), x: 0.5 - w / 2.0, y: SECTION_TITLE_Y, shadow: false, width: Some(w * SCREEN.0), height: Some(2.0 * HALF_BIG * SCREEN.1), plain: true });
                let buttons = if s == Section::Buy { self.buttons } else { frame >= self.opened_at + BUILD_BUTTONS_DELAY };
                if buttons {
                    button(out, text.live(live::SELECT), SELECT_AT);
                    button(out, text.live(live::BACK), BACK_AT);
                }
            }
        }
    }

    /// The goals list's rows.
    fn goal_texts(&self, view: &PlayerView, text: &GameText, font: &Font, out: &mut Vec<Text>) {
        let goals = self.shown_goals(view, text);
        let selected = self.goal.min(goals.len().saturating_sub(1));
        let first = (selected + 1).saturating_sub(GOAL_ROWS);
        for (slot, g) in goals.iter().enumerate().skip(first).take(GOAL_ROWS) {
            let row = slot - first;
            if slot == selected {
                for (j, l) in wrap(font, g, GOAL_WRAP_BIG).iter().enumerate() {
                    let w = width(font, l, Face::Big);
                    let y2 = GOAL_SELECTED_Y + row as f32 * GOAL_ROW_PITCH + j as f32 * GOAL_SELECTED_PITCH;
                    line(out, l, GOAL_CENTRE - w / SCREEN.0 / 2.0, y2 - HALF_SMALL, Some(w), Some(2.0 * HALF_SMALL * SCREEN.1));
                }
            } else {
                for (j, l) in wrap(font, g, GOAL_WRAP_SMALL).iter().enumerate() {
                    let w = width(font, l, Face::Small);
                    let y = GOAL_ROW_TOP + (row as f32 - 1.0) * GOAL_ROW_PITCH + j as f32 * GOAL_LINE_PITCH;
                    out.push(Text { text: l.clone(), x: GOAL_CENTRE - w / SCREEN.0 / 2.0, y, shadow: false, width: Some(w), height: Some(2.0 * HALF_SMALL * SCREEN.1), plain: true });
                }
            }
        }
    }

    /// The dialog's text block: lines broken as the game does (the quit warning at its double
    /// spaces' sentences), left aligned at the block's left edge, the block centred.
    fn dialog_lines(&self, s: Section, text: &GameText, font: &Font, out: &mut Vec<Text>) {
        let lines: Vec<String> = if s == Section::Save {
            vec![text.live(live::SAVE_QUESTION).to_string()]
        } else {
            wrap(font, text.live(live::QUIT_WARNING), DIALOG_WRAP)
        };
        let block = lines.iter().map(|l| width(font, l, Face::Dialog)).fold(0.0, f32::max);
        for (k, l) in lines.iter().enumerate() {
            let w = width(font, l, Face::Dialog);
            let x = 0.5 - block / SCREEN.0 / 2.0;
            let y = DIALOG_TEXT_TOP + if k == 0 { 0.0 } else { DIALOG_FIRST_PITCH + (k - 1) as f32 * DIALOG_LINE_PITCH };
            line(out, l, x, y, Some(w), Some(2.0 * HALF_SMALL * SCREEN.1));
        }
    }
}

/// A text broken into lines of at most `limit` glyph columns, a word at a time (single spaces
/// between words; the strings' double spaces keep an empty word between).
fn wrap(font: &Font, text: &str, limit: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(l) if columns(font, &format!("{l} {word}")) <= limit => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt::ui::{keys, pad};

    #[test]
    fn the_pad_reaches_the_movies_two_frames_later() {
        let mut p = Pause::new(Settings::default());
        p.pad(pad::RIGHT, 100);
        assert!(p.due(101).is_empty());
        assert_eq!(p.due(102), vec![Press::Key(keys::RIGHT)]);
    }

    #[test]
    fn b_is_the_triangles_key_on_the_main_row_and_a_press_of_the_game_in_a_section() {
        let mut p = Pause::new(Settings::default());
        p.pad(pad::B, 10);
        assert_eq!(p.due(12), vec![Press::Key(keys::TRIANGLE)]);
        p.focus = Focus::Section(Section::Goals);
        p.pad(pad::B, 20);
        assert_eq!(p.due(22), vec![Press::Back]);
        // the catalogs take longer to give the main row back
        p.focus = Focus::Section(Section::Buy);
        p.pad(pad::B, 30);
        assert!(p.due(37).is_empty());
        assert_eq!(p.due(38), vec![Press::Back]);
    }

    #[test]
    fn the_options_screen_reads_the_pad_in_the_frame_and_keeps_what_was_set() {
        let mut p = Pause::new(Settings::default());
        p.focus = Focus::Section(Section::Options);
        p.pad(pad::RIGHT, 50);
        p.pad(pad::DOWN, 50);
        assert_eq!(p.due(50).len(), 2);
        // the first row is free will: right toggled it; B puts it back
        assert!(!p.settings().free_will);
        p.leave_options(false);
        assert!(p.settings().free_will);
    }

    #[test]
    fn the_volumes_stay_within_their_range() {
        let mut s = Settings::default();
        s.step(4, 1);
        assert_eq!(s.sound_volume, VOLUME_MAX);
        for _ in 0..20 {
            s.step(5, -1);
        }
        assert_eq!(s.music_volume, 0);
    }

    #[test]
    fn a_section_is_named_by_its_command() {
        assert_eq!(Section::of_command("draw_career"), Some(Section::Careers));
        assert_eq!(Section::of_command("draw_main"), None);
    }

    #[test]
    fn the_calls_of_taking_the_main_row_back() {
        let names = |s: Section| Pause::close_calls(s).iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(Section::Goals), ["showMainPause", "DPadUp", "DPadDown", "DPadLeft", "DPadRight", "destroyButton"]);
        assert_eq!(names(Section::Buy).iter().filter(|n| **n == "showMainPause").count(), 2);
        assert_eq!(names(Section::Save)[0], "hideDialog");
    }
}
