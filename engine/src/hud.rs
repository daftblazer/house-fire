//! What the game feeds the HUD's and the dialog's Apt movies while a lot is played: the calls it
//! makes on the movies, the answers it gives their reads and the lines of text it draws over
//! them (`docs/simulation.md`, "The game's feed (U3)"). Simulation side: one `Hud` per player,
//! worked out from that player's Sim and the lot's clock and funds, nothing from the movies but
//! the commands they send back (`Hud::command`), and nothing drawn.
//!
//! The frame is the game's presentation frame, not a tick: the front end calls `update` and then
//! `draw` once a frame, with the world as it stands.

use crate::apt::font::Font;
use crate::motives::motive;
use crate::world::World;

/// One call of a function of the screen's movie (`Apt_CallMovie`, 0x80106164).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: &'static str,
    pub args: Vec<String>,
}

fn call(name: &'static str) -> Call {
    Call { name, args: Vec::new() }
}

/// One string the game draws (`ERFont::DoDraw`, 0x8003CB10): its top left as fractions of the
/// 640 by 448 screen, and whether it is the black shadow pass drawn first, 2 pixels down and
/// right of the face.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub shadow: bool,
    /// The line's width in pixels as the game measured it (the aligned point's twice distance)
    /// and, for a line the game puts its middle on a y, its height (pixels).
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// A plain draw: no shadow pass, the dark title colour (the pause screen's title).
    pub plain: bool,
}

/// The game's speed setting (the manager's +76 and its pause tests, `fn_80088890`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Speed {
    #[default]
    Normal,
    Paused,
    Fast,
    Fastest,
}

/// A dialog the game shows over the lot: its title, its text lines as the game breaks them and
/// the number of buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub title: String,
    pub lines: Vec<String>,
    pub buttons: usize,
    pub button_label: String,
}

impl Dialog {
    /// The dialog a Dialog node (opcode 36) opens: its operand's byte 2 is the text's place in
    /// the object's STR# 301 (counted from 1), bytes 3 and 4 the buttons' labels (0: none; no
    /// button at all makes one "OK"), byte 6 the title's. The strings' tokens are filled from
    /// the opening frame's locals (`name` gives an object's name), the text is broken into lines
    /// by `measure`. The strings hold Windows-1252's ellipsis as U+0085.
    pub fn opened(strings: &[String], operand: [u8; 8], locals: &[i16], name: &dyn Fn(i16) -> String, measure: &Measure) -> Dialog {
        let get = |i: u8| i.checked_sub(1).and_then(|i| strings.get(usize::from(i))).filter(|t| !t.is_empty()).map(|t| crate::replay::dialog_tokens(t, locals, name).replace('\u{85}', "\u{2026}"));
        let labels: Vec<String> = [3, 4].iter().filter_map(|&b| get(operand[b])).collect();
        let buttons = labels.len().max(1);
        Dialog {
            title: get(operand[6]).unwrap_or_default(),
            lines: measure.wrap(&get(operand[2]).unwrap_or_default()),
            buttons,
            button_label: labels.into_iter().next().unwrap_or_else(|| "OK".to_string()),
        }
    }
}

/// Where the movies say their bars and buttons are, as fractions of the screen: the commands
/// `money_bar_x`, `time_bar_y`, `dialog_title_bar_x`, `dialog_button_accept_y`, ...
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub money: (f32, f32),
    pub time: (f32, f32),
    pub title: (f32, f32),
    pub accept: (f32, f32),
    pub decline: (f32, f32),
}

impl Default for Layout {
    /// Before a movie reports a place: the dialog's buttons are at (-1, -1) (the pause screen's
    /// dialogs draw their labels there until their movie says where, frame 2604 of pause-c.log).
    fn default() -> Layout {
        Layout { money: (0.0, 0.0), time: (0.0, 0.0), title: (0.0, 0.0), accept: (-1.0, -1.0), decline: (-1.0, -1.0) }
    }
}

/// What the HUD shows of the lot for one player: their Sim's motives (the overall mood is
/// entry 3 of the Sim's table), the clock and the funds.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerView {
    pub motives: [f32; 16],
    pub hour: i32,
    pub minute: i32,
    pub funds: i32,
    /// The lot's house (its global 10) and, for each of that house's goals in its list's order,
    /// whether the player has met it (the pause screen's goals list).
    pub house: usize,
    pub goals_met: Vec<bool>,
    /// The levels the household's Sims have reached in the seven careers the pause screen lists.
    /// The engine has no careers yet: all 0.
    pub career_levels: [i32; 7],
}

/// The lot's global with the funds (the HUD's money text reads the game's account 0,
/// `fn_800BC920`; the lot's global 13 holds 1,030 in the home lot, as the HUD shows).
/// Unverified: that this global is the account the manager's getter reads.
const GLOBAL_FUNDS: usize = 13;

/// The lot's global with the house's number (the goals' lists are by it).
const GLOBAL_HOUSE: usize = 10;

impl PlayerView {
    /// The view of the Sim `sim` of the world; `None` if there is no such Sim.
    pub fn of(world: &World, sim: i16) -> Option<PlayerView> {
        let o = world.find_object(sim).filter(|o| o.person)?;
        let mut motives = [0.0; 16];
        for (m, v) in motives.iter_mut().zip(&o.motives) {
            *m = *v;
        }
        let g = |i: usize| i32::from(world.globals.get(i).copied().unwrap_or(0));
        let house = usize::try_from(g(GLOBAL_HOUSE)).unwrap_or(0);
        let goals_met = world.goals.houses.get(house).map(|marks| marks.iter().map(|&m| world.goals.is_met(m)).collect()).unwrap_or_default();
        Some(PlayerView { motives, hour: g(0), minute: g(5), funds: g(GLOBAL_FUNDS) - world.funds_spent, house, goals_met, career_levels: [0; 7] })
    }

    /// The overall mood the HUD's meter shows (`fn_80088890`: entry 3 of the motives, cut to a
    /// whole number).
    pub fn mood(&self) -> i32 {
        self.motives[MOOD] as i32
    }
}

/// The overall mood's place in the motive table (the HUD reads +12 of it as floats).
const MOOD: usize = 3;

/// The motives the alert test looks at: hunger, hygiene, energy, social, comfort, bladder, fun
/// and room (`Screen_Read` 0x8008BFDC, "ma": the offsets 28, 32, 20, 56, 24, 36, 60, 52 of the
/// table, a float each).
const ALERT_MOTIVES: [usize; 8] = [motive::HUNGER, motive::HYGIENE, motive::ENERGY, motive::SOCIAL, motive::COMFORT, motive::BLADDER, motive::FUN, 13];

/// A motive is in alert when its value as a share of the range -100..100 is at most this
/// (a float at 0x802A4298).
const ALERT_SHARE: f32 = 0.05;

/// The player's HUD and its dialog.
#[derive(Clone, Debug)]
pub struct Hud {
    player: u8,
    shown: bool,
    /// Play has begun (`show` was called): the speed buttons answer the setting even while the
    /// pause screen has the HUD hidden.
    started: bool,
    /// The HUD comes back from the pause screen: `showHUD` is called again (no player mode).
    reshow: bool,
    /// The key the dialog's movie reported (`dialog_key_press`): "accept" or "decline".
    dialog_key: Option<String>,
    mood_shown: i32,
    speed_shown: Speed,
    dialog: Option<Dialog>,
    dialog_event: Option<bool>,
    hud_ready: bool,
    dialog_ready: bool,
    button_ready: bool,
    layout: Layout,
}

/// The pixels of the screen a fraction is measured in.
pub const SCREEN: (f32, f32) = (640.0, 448.0);

/// The black pass of a line is drawn this many pixels down and right of its face.
const SHADOW_PIXELS: f32 = 2.0;

/// The clock's text sits left of the time bar by this much, and "AM" right of it (floats at
/// 0x802AABC0 and 0x802AABC4).
const CLOCK_OFFSET: f32 = 0.048;
const AM_OFFSET: f32 = 0.016;

/// The HUD's text is the font's size variant scaled by 13 (a float at 0x802AABB0, the width
/// `fn_800BC920` gives `ERFont::SetSize`) over the variant's 24.
const HUD_SCALE: f32 = 13.0 / 24.0;

/// Texels the font's glyph columns overstate each character's advance by, in the HUD's text.
/// Fitted to the two funds strings of ui-play.log ("$ 1,030", "$ 1,025": 2.009 each), not from
/// the game's `DoDraw`.
const HUD_TRIM: f32 = 2.0;

/// A dialog line's scale (pixels a texel), the advance of its spaces (texels) and what each
/// character's advance is short of its columns by (texels).
const DIALOG_SCALE: f32 = 0.6191735;
const DIALOG_SPACE: f32 = 12.4737;
const DIALOG_PAD: f32 = -1.49015;

/// The advance of a space, in texels (`apt_draw`'s fit; the widths of the logged lines give 10.6
/// to 12).
const SPACE: f32 = 11.0;

/// The height of the HUD's text as a fraction of the screen's: a float at 0x802A3764 that is
/// within 4e-5 of the half height the log's lines give. Unverified that it is the height.
pub const TEXT_HEIGHT: f32 = 0.0355;

/// Where the dialog's window is (`Screen_Read` 0x800445EC: doubles at 0x80298FE0 and 0x80298FF0,
/// 0x80298FF8) and the text's top below its top (a float at 0x80298FE8).
pub const DIALOG_WINDOW: (f32, f32, f32) = (0.2, 0.18, 0.6);
const DIALOG_TEXT_TOP: f32 = 0.1;

/// The height the dialog's box grows by for each line of text, as a fraction of the screen. Fitted
/// to the one dialog of the logs that is read in play (Mom's three lines: `dialog_box_height`
/// 0.418405 less the 0.1 above the text), not the game's `fn_80044174`.
/// What the game's measure of the dialog's title and its button's label is of the HUD's measure
/// of the same string. Fitted to Mom's dialog, the one the logs read in play ("Mom" 35.75 pixels
/// by the HUD's measure, 56.73 read as `dialog_title_bar_width`; "OK" 22.75 and 23.79 as
/// `UI_button_width`), not the game's own measure (`fn_800441C0`).
const DIALOG_TITLE_RATIO: f32 = 56.727 / 35.75;
const DIALOG_BUTTON_RATIO: f32 = 23.788 / 22.75;

/// How much bigger than the HUD's lines the dialog's title and its button's label are drawn
/// (width and height, from the size words of Mom's dialog: "Mom" 50.5 by 22.1 pixels against the
/// HUD measure's 35.75 by 15.9, "OK" 25.8 by 18.4 against 22.75 by 15.9). Fitted to that one
/// dialog, not the game's `SetSize`.
const DIALOG_TITLE_DRAW: (f32, f32) = (50.5 / 35.75, 22.1 / 15.904);
const DIALOG_LABEL_DRAW: (f32, f32) = (25.8 / 22.75, 18.4 / 15.904);

const DIALOG_LINE_HEIGHT: f32 = 0.318405 / 3.0;

/// The point the dialog's lines are centred on is a pixel right of the window's middle (the
/// second point of the size words of Mom's lines, 0.5015625). Not from the game; the other
/// dialogs of the log have other shifts (0.4981, 0.495, 0.5).
const DIALOG_CENTRE_SHIFT: f32 = 1.0 / 640.0;

/// The dialog's lines are broken at this width in pixels. Not from the game: any width from the
/// longest line of Mom's dialog (275) to the first that would not fit (321) breaks it as the log
/// has it.
pub const WRAP_WIDTH: f32 = 300.0;

/// The scale of a plain draw (no size words), pixels a texel. Fitted to the width of "Customize A
/// Sim" in frames 1050 and 1122 of ui-dump.log (sweep 0.58 to 0.64 on the within-4 share).
pub const PLAIN_SCALE: f32 = 0.62;

/// The line the game's text is measured in: texels of the font's pages.
pub struct Measure<'a> {
    pub font: &'a Font,
}

impl Measure<'_> {
    /// The width in pixels of a HUD line.
    pub fn hud_width(&self, text: &str) -> f32 {
        let texels: f32 = text.chars().map(|c| if c == ' ' { SPACE } else { self.font.glyph(c as u32).map_or(0.0, |g| (g.x1 - g.x0) as f32) } - HUD_TRIM).sum();
        texels * HUD_SCALE
    }

    /// The width in pixels of a dialog's line: the page's texels scaled, a space a texel count of
    /// its own and each character's advance short of its columns by `DIALOG_PAD`. The three
    /// numbers are the solution of the widths of the three lines of Mom's greeting in the
    /// log (not from the game's `DoDraw`; the autosave dialog's lines fit within 0.2 pixels).
    pub fn dialog_width(&self, text: &str) -> f32 {
        let texels: f32 = text.chars().map(|c| if c == ' ' { DIALOG_SPACE } else { self.font.glyph(c as u32).map_or(0.0, |g| (g.x1 - g.x0) as f32) } + DIALOG_PAD).sum();
        texels * DIALOG_SCALE
    }

    /// The width in pixels of a plain draw (no shadow pass: the interaction menu's labels): the
    /// glyph columns at `PLAIN_SCALE`.
    pub fn plain_width(&self, text: &str) -> f32 {
        text.chars().map(|c| if c == ' ' { SPACE } else { self.font.glyph(c as u32).map_or(0.0, |g| (g.x1 - g.x0) as f32) }).sum::<f32>() * PLAIN_SCALE
    }

    /// The text broken into lines at `WRAP_WIDTH` pixels, a word at a time.
    pub fn wrap(&self, text: &str) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for word in text.split_whitespace() {
            match lines.last_mut() {
                Some(l) if self.dialog_width(&format!("{l} {word}")) <= WRAP_WIDTH => {
                    l.push(' ');
                    l.push_str(word);
                }
                _ => lines.push(word.to_string()),
            }
        }
        lines
    }
}

impl Hud {
    /// The HUD of player `player` (1 or 2), not shown yet.
    pub fn new(player: u8) -> Hud {
        Hud { player, shown: false, started: false, reshow: false, dialog_key: None, mood_shown: 0, speed_shown: Speed::Normal, dialog: None, dialog_event: None, hud_ready: false, dialog_ready: false, button_ready: false, layout: Layout::default() }
    }

    /// The HUD is shown (a lot's play begins): it keeps the mood it shows from now.
    pub fn show(&mut self, view: &PlayerView) {
        self.shown = true;
        self.started = true;
        self.mood_shown = view.mood();
    }

    /// The pause screen takes the HUD's place (`hideHUD`): its layers and the lot's clock stop
    /// being drawn, and the HUD movie says `hud_ready` again when it is back.
    pub fn pause(&mut self) {
        self.shown = false;
        self.hud_ready = false;
    }

    /// The pause screen is gone: the HUD is shown again (`showHUD`, 0x80087F08, without the
    /// player mode: the log has none at the pause's end).
    pub fn resume(&mut self, view: &PlayerView) {
        self.shown = true;
        self.reshow = true;
        self.mood_shown = view.mood();
    }

    /// The layout the movies reported (the places of the bars and the dialog's title and button).
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Whether a dialog is up.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// The dialog's button the player's key chose, if the dialog's movie reported one
    /// (`dialog_key_press`: "accept" or "decline").
    pub fn take_dialog_key(&mut self) -> Option<String> {
        self.dialog_key.take()
    }

    /// A dialog opens over the lot.
    pub fn show_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.dialog_event = Some(true);
    }

    /// The dialog is answered and goes.
    pub fn hide_dialog(&mut self) {
        if self.dialog.take().is_some() {
            self.dialog_event = Some(false);
            self.dialog_ready = false;
            self.button_ready = false;
        }
    }

    /// A command a movie sent the game (`Screen_Command`, 0x8008D190 and 0x80044350).
    pub fn command(&mut self, name: &str, argument: &str) {
        let v = argument.parse::<f32>().unwrap_or(0.0);
        match name {
            "hud_ready" => self.hud_ready = argument == "1",
            "dialog_ready" => self.dialog_ready = argument == "1",
            "dialog_key_press" => self.dialog_key = Some(argument.to_string()),
            "money_bar_x" => self.layout.money.0 = v,
            "money_bar_y" => self.layout.money.1 = v,
            "time_bar_x" => self.layout.time.0 = v,
            "time_bar_y" => self.layout.time.1 = v,
            "dialog_title_bar_x" => self.layout.title.0 = v,
            "dialog_title_bar_y" => self.layout.title.1 = v,
            "dialog_button_accept_x" => self.layout.accept.0 = v,
            "dialog_button_accept_y" => self.layout.accept.1 = v,
            "dialog_button_decline_x" => self.layout.decline.0 = v,
            "dialog_button_decline_y" => self.layout.decline.1 = v,
            "dialog_button_accept_ready" => self.button_ready = argument == "true",
            _ => {}
        }
    }

    /// The calls of the frame's update part, in the order the game makes them: the HUD shown,
    /// the dialog opened or closed, the speed buttons, the mood meter.
    pub fn update(&mut self, view: &PlayerView, speed: Speed, first_frame: bool) -> Vec<Call> {
        let mut out = Vec::new();
        if !self.started {
            return out;
        }
        if self.shown && first_frame {
            // showHUD (0x80087F08), then setPlayerMode with the number of players (0x8008800C)
            out.push(call("showHUD"));
            out.push(Call { name: "setPlayerMode", args: vec!["1p".to_string()] });
        }
        if self.shown && std::mem::take(&mut self.reshow) {
            out.push(call("showHUD"));
        }
        match self.dialog_event.take() {
            Some(true) => out.push(call("showDialog")),
            Some(false) => out.push(call("hideDialog")),
            None => {}
        }
        if speed != self.speed_shown {
            // 0x80088E78 and its neighbours: the other two buttons hidden, the new one shown
            let (hide, show): ([&str; 2], &str) = match speed {
                Speed::Paused => (["hideFastForward", "hideSuperFastForward"], "showRuntimePause"),
                Speed::Fast => (["hideRuntimePause", "hideSuperFastForward"], "showFastForward"),
                Speed::Fastest => (["hideFastForward", "hideRuntimePause"], "showSuperFastForward"),
                Speed::Normal => (["hideRuntimePause", "hideFastForward"], "hideSuperFastForward"),
            };
            out.extend(hide.into_iter().map(call));
            out.push(call(show));
            self.speed_shown = speed;
        }
        let mood = view.mood();
        if self.shown && mood != self.mood_shown {
            // updateHUD with the player's number and the meter (0x8008893C)
            out.push(Call { name: "updateHUD", args: vec![self.player.to_string(), "updateMoodMeter".to_string()] });
            self.mood_shown = mood;
        }
        out
    }

    /// The calls and lines of the frame's draw part: the HUD's layers and its funds, clock and
    /// "AM"/"PM", then the dialog's.
    pub fn draw(&self, view: &PlayerView, measure: &Measure) -> (Vec<Call>, Vec<Text>) {
        let mut calls = Vec::new();
        let mut texts = Vec::new();
        if self.shown && self.hud_ready {
            // 0x80089D7C: the content layer between the dialog layer's hide and show
            calls.extend(["hideDialogLayer", "showContentLayer", "showDialogLayer"].map(call));
        }
        if self.dialog.is_some() {
            // 0x8008A28C
            calls.extend(["hideContentLayer", "showDialogLayer", "showContentLayer"].map(call));
        }
        if self.shown && self.hud_ready {
            let h = TEXT_HEIGHT;
            let hh = Some(h * SCREEN.1);
            self.funds_text(view, measure, &mut texts);
            let hour = if view.hour % 12 == 0 { 12 } else { view.hour % 12 };
            line(&mut texts, &format!("{hour}:{:02}", view.minute), self.layout.time.0 - CLOCK_OFFSET, self.layout.time.1 - h / 2.0, None, hh);
            line(&mut texts, if view.hour > 11 { "PM" } else { "AM" }, self.layout.time.0 + AM_OFFSET, self.layout.time.1 - h / 2.0, None, hh);
        }
        if let Some(d) = self.dialog.as_ref().filter(|_| self.dialog_ready) {
            let (wx, wy, ww) = DIALOG_WINDOW;
            let h = TEXT_HEIGHT;
            let centred = |s: &str| wx + ww / 2.0 + DIALOG_CENTRE_SHIFT - measure.dialog_width(s) / SCREEN.0 / 2.0;
            let (tw, th) = (measure.hud_width(&d.title) * DIALOG_TITLE_DRAW.0, h * DIALOG_TITLE_DRAW.1);
            line(&mut texts, &d.title, self.layout.title.0 - tw / SCREEN.0 / 2.0, self.layout.title.1 - th / 2.0, Some(tw), Some(th * SCREEN.1));
            for (k, l) in d.lines.iter().enumerate() {
                line(&mut texts, l, centred(l), wy + DIALOG_TEXT_TOP + k as f32 * 20.0 / SCREEN.1, Some(measure.dialog_width(l)), None);
            }
            if self.button_ready {
                let (bw, bh) = (measure.hud_width(&d.button_label) * DIALOG_LABEL_DRAW.0, h * DIALOG_LABEL_DRAW.1);
                line(&mut texts, &d.button_label, self.layout.accept.0 - bw / SCREEN.0 / 2.0, self.layout.accept.1 - bh / 2.0, Some(bw), Some(bh * SCREEN.1));
            }
        }
        (calls, texts)
    }

    /// The funds line ("$ 1,030"), centred on the money bar's place (the pause screen draws it too).
    pub fn funds_text(&self, view: &PlayerView, measure: &Measure, out: &mut Vec<Text>) {
        self.funds_line(view.funds, self.layout.money, measure, out);
    }

    /// An amount of money as a funds line centred on a place.
    pub fn funds_line(&self, amount: i32, at: (f32, f32), measure: &Measure, out: &mut Vec<Text>) {
        let funds = format!("$ {}", thousands(amount));
        let w = measure.hud_width(&funds) / SCREEN.0;
        line(out, &funds, at.0 - w / 2.0, at.1 - TEXT_HEIGHT / 2.0, Some(w * SCREEN.0), Some(TEXT_HEIGHT * SCREEN.1));
    }

    /// The answer of the HUD's handler (`Screen_Read`, 0x8008BFDC) to a movie's read of a game
    /// value by name; `None` for a name it does not answer (the dialog's).
    pub fn answer(&self, name: &str, view: &PlayerView) -> Option<String> {
        let flag = |b: bool| if b { "true" } else { "false" }.to_string();
        Some(match name {
            "om" => view.mood().to_string(),
            "ma" => flag(ALERT_MOTIVES.iter().any(|&i| ((view.motives[i] + 100.0) / 200.0).clamp(0.0, 1.0) <= ALERT_SHARE)),
            // the relationship scan (0x800B830C, 0x800B80CC) and the skill gain test are not
            // native yet: no alert (docs/open.md)
            "ra" | "ct" => flag(false),
            "two_player" => "0".to_string(),
            _ => return None,
        })
    }

    /// The answer of the dialog's handler (`Screen_Read`, 0x800445EC).
    pub fn dialog_answer(&self, name: &str, measure: &Measure) -> Option<String> {
        let d = self.dialog.as_ref()?;
        let (x, y, w) = DIALOG_WINDOW;
        Some(match name {
            "dialog_x" => format!("{x:.6}"),
            "dialog_y" => format!("{y:.6}"),
            "dialog_box_width" => format!("{w:.6}"),
            "number_of_buttons" => d.buttons.to_string(),
            "is_critical_dialog" => "0".to_string(),
            // the game's own measure of its strings (`fn_80044174`, `fn_800441C0`, the title's
            // `ERFont::DoGetStringSize`): the same measure as the lines' widths, in the screen's
            // fractions
            "dialog_title_bar_width" => format!("{:.6}", measure.hud_width(&d.title) * DIALOG_TITLE_RATIO / SCREEN.0),
            "UI_button_width" => format!("{:.6}", measure.hud_width(&d.button_label) * DIALOG_BUTTON_RATIO / SCREEN.0),
            "dialog_box_height" => format!("{:.6}", DIALOG_TEXT_TOP + d.lines.len() as f32 * DIALOG_LINE_HEIGHT),
            _ => return None,
        })
    }
}

/// A line and its shadow pass.
pub fn line(out: &mut Vec<Text>, text: &str, x: f32, y: f32, width: Option<f32>, height: Option<f32>) {
    out.push(Text { text: text.to_string(), x: x + SHADOW_PIXELS / SCREEN.0, y: y + SHADOW_PIXELS / SCREEN.1, shadow: true, width, height, plain: false });
    out.push(Text { text: text.to_string(), x, y, shadow: false, width, height, plain: false });
}

/// A number with its thousands apart by commas.
pub fn thousands(n: i32) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_set_apart() {
        assert_eq!(thousands(1030), "1,030");
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(-9000), "-9,000");
    }

    fn view(mood: f32) -> PlayerView {
        let mut motives = [0.0; 16];
        motives[MOOD] = mood;
        PlayerView { motives, hour: 7, minute: 4, funds: 1030, house: 1, goals_met: Vec::new(), career_levels: [0; 7] }
    }

    #[test]
    fn the_mood_meter_updates_when_the_whole_number_changes() {
        let mut h = Hud::new(1);
        h.show(&view(19.4));
        assert!(h.update(&view(19.1), Speed::Normal, false).is_empty());
        let c = h.update(&view(18.9), Speed::Normal, false);
        assert_eq!(c, vec![Call { name: "updateHUD", args: vec!["1".into(), "updateMoodMeter".into()] }]);
    }

    #[test]
    fn a_speed_change_hides_the_other_buttons_and_shows_its_own() {
        let mut h = Hud::new(1);
        h.show(&view(0.0));
        let names: Vec<_> = h.update(&view(0.0), Speed::Fast, false).iter().map(|c| c.name).collect();
        assert_eq!(names, ["hideRuntimePause", "hideSuperFastForward", "showFastForward"]);
        let names: Vec<_> = h.update(&view(0.0), Speed::Normal, false).iter().map(|c| c.name).collect();
        assert_eq!(names, ["hideRuntimePause", "hideFastForward", "hideSuperFastForward"]);
    }

    #[test]
    fn alerts_follow_the_lowest_motives() {
        let h = Hud::new(1);
        let mut v = view(0.0);
        assert_eq!(h.answer("ma", &v).as_deref(), Some("false"));
        for m in ALERT_MOTIVES {
            v.motives[m] = 10.0;
        }
        assert_eq!(h.answer("ma", &v).as_deref(), Some("false"));
        v.motives[motive::BLADDER] = -91.0;
        assert_eq!(h.answer("ma", &v).as_deref(), Some("true"));
    }

    #[test]
    fn the_pause_screen_takes_the_hud_and_gives_it_back_with_showhud_alone() {
        let mut h = Hud::new(1);
        h.show(&view(0.0));
        assert_eq!(h.update(&view(0.0), Speed::Normal, true).iter().map(|c| c.name).collect::<Vec<_>>(), ["showHUD", "setPlayerMode"]);
        h.pause();
        // the speed buttons still follow the setting, the mood meter does not
        let names: Vec<_> = h.update(&view(9.0), Speed::Paused, false).iter().map(|c| c.name).collect();
        assert_eq!(names, ["hideFastForward", "hideSuperFastForward", "showRuntimePause"]);
        h.resume(&view(9.0));
        let names: Vec<_> = h.update(&view(9.0), Speed::Normal, false).iter().map(|c| c.name).collect();
        assert_eq!(names, ["showHUD", "hideRuntimePause", "hideFastForward", "hideSuperFastForward"]);
    }

    #[test]
    fn the_dialogs_key_press_is_taken_once() {
        let mut h = Hud::new(1);
        h.command("dialog_key_press", "accept");
        assert_eq!(h.take_dialog_key().as_deref(), Some("accept"));
        assert_eq!(h.take_dialog_key(), None);
    }
}
