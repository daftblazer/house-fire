//! The Apt player: loaded movies, their timelines and clips, the script machine (`vm.rs`), the
//! commands the movies send the game, and each frame's display list (`display.rs`).
//!
//! The player does not draw. It is driven the way the game drives its movies: the game loads
//! a movie by path (`load`), calls a function of the movie by name (`call`), and the movies
//! run one frame at a time (`advance`). A movie asks for another movie to be loaded into one of
//! its clips (`loadMovie`); the request waits until the game loads that movie, as the game's own
//! loads come some frames after the request. A movie sends the game a command by writing a
//! member of the object `extern`.
//!
//! Frames count from 0 here. Every clip is a node of one tree (shapes, texts and buttons are
//! leaves); a clip's frame items are applied when its frame is entered, and the frame's actions
//! run afterwards, in the order the clips entered their frames.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::rc::Rc;

use super::Flash;
use super::actions::Value as Const;
use super::movie::{Character, Frame, Item, place};
use super::value::{ClipId, Kind, Native, ObjRef, Object, Props, Value};

/// A command a movie sent the game: what the game's handler is given.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// The game frame the player was told it was at (`Player::frame`).
    pub frame: u32,
    pub name: String,
    pub argument: String,
}

/// What a clip shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    /// A movie or sprite: a timeline with children.
    Sprite,
    Shape,
    Image,
    Text,
    Button,
}

/// A colour transform: the multiplier and the offset of red, green, blue, alpha (0 to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub mul: [f64; 4],
    pub add: [f64; 4],
}

impl Color {
    pub const IDENTITY: Color = Color { mul: [1.0; 4], add: [0.0; 4] };

    /// A child's colour inside this one.
    pub fn then(&self, c: &Color) -> Color {
        let mut o = Color::IDENTITY;
        for i in 0..4 {
            o.mul[i] = self.mul[i] * c.mul[i];
            o.add[i] = self.add[i] + self.mul[i] * c.add[i];
        }
        o
    }
}

/// An affine matrix (a, b, c, d, tx, ty) as the Flash format has it.
pub type Matrix = [f64; 6];
pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `p` after `c`: the matrix of a child (`c`) inside a parent (`p`).
pub fn compose(p: &Matrix, c: &Matrix) -> Matrix {
    [
        p[0] * c[0] + p[2] * c[1],
        p[1] * c[0] + p[3] * c[1],
        p[0] * c[2] + p[2] * c[3],
        p[1] * c[2] + p[3] * c[3],
        p[0] * c[4] + p[2] * c[5] + p[4],
        p[1] * c[4] + p[3] * c[5] + p[5],
    ]
}

pub struct Clip {
    pub flash: Rc<Flash>,
    /// The character of `flash` (0 for the movie itself).
    pub ch: u32,
    pub kind: ClipKind,
    pub parent: Option<ClipId>,
    pub name: String,
    pub depth: i32,
    pub children: BTreeMap<i32, ClipId>,
    /// The variables set on the clip.
    pub props: Props,
    pub frame: usize,
    pub playing: bool,
    pub matrix: Matrix,
    pub color: Color,
    pub visible: bool,
    pub alive: bool,
    pub clip_depth: i32,
    /// The frame of its parent's timeline that placed it.
    pub placed: usize,
    /// The character the timeline placed (`ch` changes when a movie is loaded into the clip).
    pub placed_ch: u32,
    /// The name of the movie loaded into the clip by `load`, if one is.
    pub loaded: Option<String>,
    /// A text field's contents once a script set them.
    pub text: Option<Rc<str>>,
}

/// An action stream waiting to run: where it is in its movie, and the clip it belongs to.
struct Pending {
    clip: ClipId,
    flash: Rc<Flash>,
    ch: u32,
    frame: usize,
    item: usize,
}

/// A request of a movie to load another into a clip.
#[derive(Clone, Debug)]
struct LoadRequest {
    /// The movie's name, lower case, without folder and extension.
    name: String,
    target: ClipId,
}

pub struct Player {
    flashes: HashMap<String, Rc<Flash>>,
    pub(super) clips: Vec<Clip>,
    pub root: Option<ClipId>,
    pub(super) extern_obj: ObjRef,
    pub(super) globals: Props,
    pub commands: Vec<Command>,
    queue: VecDeque<Pending>,
    requests: VecDeque<LoadRequest>,
    /// Movies on their way into clips: (game frame they start at, name, clip).
    scheduled: Vec<(u32, String, ClipId)>,
    /// Game frames from a movie's request to its start when the game has the movie in memory,
    /// and from the game's logged load to the start (measured against the commands'
    /// frames, `docs/assets.md`).
    pub cache_delay: u32,
    pub load_delay: u32,
    /// The game's frame counter, stamped on the commands.
    pub frame: u32,
    /// What was asked of the player that it does not do (each kind once).
    pub stubs: Vec<String>,
    /// The key the game reports to `Key.getCode`.
    pub key_code: f64,
    /// How deep scripts have called into each other (a guard against runaway scripts).
    pub(super) depth: u32,
    /// The instructions run since the last `advance` or `call` (a guard).
    pub(super) steps: u64,
    pub(super) rng: u32,
    /// Prints each action run (`APT_TRACE`).
    pub trace: bool,
    /// Prints each member and local variable a script sets (`APT_TRACE=2`).
    pub(super) trace_sets: bool,
}

/// The depth at which a script creates clips (below it only the timeline places).
pub const SCRIPT_DEPTH: i32 = 16384;

impl Player {
    /// A player over movies by name (any case; `flashes.arc`'s bundles).
    pub fn new(flashes: impl IntoIterator<Item = (String, Rc<Flash>)>) -> Player {
        let mut p = Player {
            flashes: flashes.into_iter().map(|(n, f)| (n.to_ascii_lowercase(), f)).collect(),
            clips: Vec::new(),
            root: None,
            extern_obj: Object::new(Kind::Native(Native::Extern)),
            globals: Props::default(),
            commands: Vec::new(),
            queue: VecDeque::new(),
            requests: VecDeque::new(),
            scheduled: Vec::new(),
            cache_delay: 3,
            load_delay: 2,
            frame: 0,
            stubs: Vec::new(),
            key_code: 0.0,
            depth: 0,
            steps: 0,
            rng: 1,
            trace: std::env::var_os("APT_TRACE").is_some(),
            trace_sets: std::env::var("APT_TRACE").is_ok_and(|v| v == "2"),
        };
        p.init_globals();
        p
    }

    /// A value the game provides to the movies (a member of `extern`).
    pub fn set_extern(&mut self, name: &str, v: Value) {
        self.extern_obj.borrow_mut().props.set(name, v);
    }

    pub fn note_stub(&mut self, what: String) {
        if !self.stubs.contains(&what) {
            self.stubs.push(what);
        }
    }

    pub fn clip(&self, id: ClipId) -> &Clip {
        &self.clips[id]
    }

    /// The name a movie path is known by: no folder, no extension, lower case.
    fn stem(path: &str) -> String {
        let base = path.rsplit(['\\', '/']).next().unwrap_or(path);
        let base = base.strip_suffix(".swf").or_else(|| base.strip_suffix(".SWF")).unwrap_or(base);
        base.to_ascii_lowercase()
    }

    /// Whether `load` of this path would load something: the first movie always, later ones when
    /// a movie has asked for them.
    pub fn can_load(&self, path: &str) -> bool {
        let name = Self::stem(path);
        self.root.is_none() || self.requests.iter().any(|r| r.name == name && self.clips[r.target].alive)
    }

    /// The game loads a movie. The first one becomes the root; later ones fill the clip a movie
    /// asked to load them into. Returns false when no movie asked for it (nothing is loaded).
    pub fn load(&mut self, path: &str) -> bool {
        let name = Self::stem(path);
        let Some(flash) = self.flashes.get(&name).cloned() else {
            self.note_stub(format!("load of {path}: no such movie"));
            return false;
        };
        if self.root.is_none() {
            let id = self.new_clip(flash, 0, ClipKind::Sprite, None, 0, String::new());
            self.root = Some(id);
            self.run_queue();
            return true;
        }
        let Some(_) = self.requests.iter().position(|r| r.name == name && self.clips[r.target].alive) else {
            let pending: Vec<String> = self.requests.iter().map(|r| format!("{}->{} alive {}", r.name, r.target, self.clips[r.target].alive)).collect();
            self.note_stub(format!("load of {path}: no movie asked for it ({})", pending.join(", ")));
            return false;
        };
        // one load of the game serves every clip that asked for the movie
        let waiting: Vec<ClipId> = self.requests.iter().filter(|r| r.name == name).map(|r| r.target).collect();
        self.requests.retain(|r| r.name != name);
        // The movie starts a little after the game's load: `load_delay` game frames (none for
        // the `f_` movies, which only hold functions the game calls at once).
        let delay = if name.starts_with("f_") { 0 } else { self.load_delay };
        if self.trace {
            eprintln!("apt: load {name} at game frame {} (start at {})", self.frame, self.frame + delay);
        }
        for t in waiting {
            if self.clips[t].alive {
                self.scheduled.push((self.frame + delay, name.clone(), t));
            }
        }
        self.tick_loads();
        true
    }

    /// Calls a function of the movies by name, as the game does: the root's, then those of the
    /// movies loaded into the root's clips. Returns whether one was found.
    pub fn call(&mut self, name: &str, args: &[Value]) -> bool {
        let Some(root) = self.root else { return false };
        let mut holders = vec![root];
        holders.extend(self.clips[root].children.values().copied());
        for id in holders {
            if let Some(Value::Obj(o)) = self.clips[id].props.get(name).cloned() {
                let f = match &o.borrow().kind {
                    Kind::Func(f) => Some(f.clone()),
                    _ => None,
                };
                if let Some(f) = f {
                    self.depth = 0;
                    self.call_func(&f, Value::Clip(id), args.to_vec());
                    self.run_queue();
                    return true;
                }
            }
        }
        false
    }

    /// One movie frame: every playing clip enters its next frame (a movie's clips in tree order,
    /// parents first), then the frames' actions run.
    pub fn advance(&mut self) {
        let Some(root) = self.root else { return };
        let mut order = Vec::new();
        self.collect(root, &mut order);
        for id in order {
            if !self.clips[id].alive || !self.clips[id].playing {
                continue;
            }
            let n = self.frame_count(id);
            if n <= 1 {
                continue;
            }
            let next = self.clips[id].frame + 1;
            self.goto(id, if next >= n { 0 } else { next }, true);
        }
        self.run_queue();
    }

    fn collect(&self, id: ClipId, out: &mut Vec<ClipId>) {
        out.push(id);
        for &c in self.clips[id].children.values() {
            self.collect(c, out);
        }
    }

    pub(super) fn frames_of(flash: &Flash, ch: u32) -> &[Frame] {
        if ch == 0 {
            return &flash.movie.frames;
        }
        match flash.movie.characters.get(ch as usize) {
            Some(Character::Sprite { frames }) => frames,
            _ => &[],
        }
    }

    pub fn frame_count(&self, id: ClipId) -> usize {
        let c = &self.clips[id];
        Self::frames_of(&c.flash, c.ch).len().max(1)
    }

    /// The frame a label names.
    pub fn label_frame(&self, id: ClipId, label: &str) -> Option<usize> {
        let c = &self.clips[id];
        Self::frames_of(&c.flash, c.ch).iter().flat_map(|f| &f.items).find_map(|i| match i {
            Item::Label { name, frame, .. } if name.eq_ignore_ascii_case(label) => Some(*frame as usize),
            _ => None,
        })
    }

    /// Makes a clip from a character, not yet in the tree and not yet in its first frame.
    fn make_clip(&mut self, flash: Rc<Flash>, ch: u32, kind: ClipKind, parent: Option<ClipId>, depth: i32, name: String) -> ClipId {
        let id = self.clips.len();
        self.clips.push(Clip {
            flash,
            ch,
            kind,
            parent,
            name,
            depth,
            children: BTreeMap::new(),
            props: Props::default(),
            frame: 0,
            playing: true,
            matrix: IDENTITY,
            color: Color::IDENTITY,
            visible: true,
            alive: true,
            clip_depth: -1,
            placed: 0,
            placed_ch: ch,
            loaded: None,
            text: None,
        });
        id
    }

    fn new_clip(&mut self, flash: Rc<Flash>, ch: u32, kind: ClipKind, parent: Option<ClipId>, depth: i32, name: String) -> ClipId {
        let id = self.make_clip(flash, ch, kind, parent, depth, name);
        if kind == ClipKind::Sprite {
            self.enter_frame(id, 0, true);
        }
        id
    }

    /// Puts a new clip of a character at a depth of `parent` (what was there goes), entering
    /// its first frame.
    pub(super) fn spawn(&mut self, parent: ClipId, flash: Rc<Flash>, ch: u32, kind: ClipKind, depth: i32, name: &str) -> ClipId {
        if let Some(old) = self.clips[parent].children.remove(&depth) {
            self.kill(old);
        }
        let id = self.make_clip(flash, ch, kind, Some(parent), depth, name.to_string());
        self.clips[parent].children.insert(depth, id);
        if kind == ClipKind::Sprite {
            self.enter_frame(id, 0, true);
        }
        id
    }

    /// Replaces what a clip holds with a movie (the clip keeps its place in its parent).
    fn fill(&mut self, id: ClipId, flash: Rc<Flash>, name: &str) {
        self.clear(id);
        let c = &mut self.clips[id];
        c.flash = flash;
        c.ch = 0;
        c.kind = ClipKind::Sprite;
        c.loaded = Some(name.to_string());
        c.props = Props::default();
        c.frame = 0;
        c.playing = true;
        self.enter_frame(id, 0, true);
    }

    /// Empties a clip: its children go, and it holds no movie.
    fn clear(&mut self, id: ClipId) {
        self.clips[id].loaded = None;
        let kids: Vec<ClipId> = std::mem::take(&mut self.clips[id].children).into_values().collect();
        for k in kids {
            self.kill(k);
        }
    }

    /// Takes a clip and all it holds out of the tree.
    pub(super) fn kill(&mut self, id: ClipId) {
        let kids: Vec<ClipId> = std::mem::take(&mut self.clips[id].children).into_values().collect();
        for k in kids {
            self.kill(k);
        }
        self.clips[id].alive = false;
        self.queue.retain(|p| p.clip != id);
    }

    /// Removes a clip from its parent and kills it.
    pub(super) fn remove_clip(&mut self, id: ClipId) {
        if let Some(p) = self.clips[id].parent {
            let d = self.clips[id].depth;
            if self.clips[p].children.get(&d) == Some(&id) {
                self.clips[p].children.remove(&d);
            }
        }
        self.kill(id);
        if self.root == Some(id) {
            self.root = None;
        }
    }

    /// Applies a frame's items to a clip: places and removes happen at once, the actions wait in
    /// the queue.
    fn enter_frame(&mut self, id: ClipId, frame: usize, actions: bool) {
        let flash = self.clips[id].flash.clone();
        let ch = self.clips[id].ch;
        let Some(f) = Self::frames_of(&flash, ch).get(frame) else { return };
        self.clips[id].frame = frame;
        // the clips placed here start before the frame's own script runs (a script finds its
        // clips, as in Flash where the script tag follows the placements)
        let mut own = Vec::new();
        for (i, item) in f.items.iter().enumerate() {
            match item {
                Item::Place(p) => self.place(id, &flash, p, frame),
                Item::Remove { depth } => {
                    if let Some(c) = self.clips[id].children.remove(depth) {
                        self.kill(c);
                    }
                }
                Item::Action(_) if actions => own.push(i),
                _ => {}
            }
        }
        for i in own {
            self.queue.push_back(Pending { clip: id, flash: flash.clone(), ch, frame, item: i });
        }
    }

    fn place(&mut self, id: ClipId, flash: &Rc<Flash>, p: &super::movie::Place, frame: usize) {
        let existing = self.clips[id].children.get(&p.depth).copied();
        if p.flags & place::HAS_CHARACTER != 0 && p.character >= 0 {
            let kind = match flash.movie.characters.get(p.character as usize) {
                Some(Character::Sprite { .. }) => Some(ClipKind::Sprite),
                Some(Character::Shape { .. }) => Some(ClipKind::Shape),
                Some(Character::Image { .. }) => Some(ClipKind::Image),
                Some(Character::Text(_)) => Some(ClipKind::Text),
                Some(Character::Button(_)) => Some(ClipKind::Button),
                _ => None,
            };
            let Some(kind) = kind else {
                self.note_stub(format!("place of character {} ({})", p.character, flash.name));
                return;
            };
            // what this frame placed before (a timeline entering its frames again) stays
            if let Some(e) = existing
                && p.flags & place::MOVE == 0
                && self.clips[e].placed_ch == p.character as u32
                && self.clips[e].placed == frame
            {
                return;
            }
            // a placed character replaces what is at the depth, taking over its matrix and
            // colour when the item sets none
            let old = existing.map(|e| (self.clips[e].matrix, self.clips[e].color, self.clips[e].name.clone()));
            if let Some(e) = existing {
                self.clips[id].children.remove(&p.depth);
                self.kill(e);
            }
            // the clip is in the tree before its first frame runs, so scripts find their parent
            let nid = self.make_clip(flash.clone(), p.character as u32, kind, Some(id), p.depth, String::new());
            if p.flags & place::MOVE != 0
                && let Some((m, c, n)) = old
            {
                self.clips[nid].matrix = m;
                self.clips[nid].color = c;
                self.clips[nid].name = n;
            }
            self.clips[id].children.insert(p.depth, nid);
            self.clips[nid].placed = frame;
            self.apply_fields(nid, p);
            if kind == ClipKind::Sprite {
                self.enter_frame(nid, 0, true);
            }
            return;
        }
        if let Some(t) = existing
            && p.flags & place::MOVE != 0
        {
            self.apply_fields(t, p);
        }
    }

    /// The fields a place item sets.
    fn apply_fields(&mut self, id: ClipId, p: &super::movie::Place) {
        let c = &mut self.clips[id];
        if p.flags & place::HAS_MATRIX != 0 {
            c.matrix = [p.matrix[0], p.matrix[1], p.matrix[2], p.matrix[3], p.translation[0], p.translation[1]].map(f64::from);
        }
        if p.flags & place::HAS_COLOR != 0 {
            // 0xAARRGGBB multiplier and 0x00RRGGBB offset (the alpha is the high byte: the
            // shadows are 0xFF000000 and fades 0xCCFFFFFF)
            let ch = |v: u32, s: u32| f64::from((v >> s) & 0xff) / 255.0;
            c.color.mul = [ch(p.color, 16), ch(p.color, 8), ch(p.color, 0), ch(p.color, 24)];
            c.color.add = [ch(p.color_add, 16), ch(p.color_add, 8), ch(p.color_add, 0), 0.0];
        }
        if p.flags & place::HAS_NAME != 0
            && let Some(n) = &p.name
        {
            c.name = n.clone();
        }
        if p.flags & place::HAS_CLIP_DEPTH != 0 {
            c.clip_depth = p.clip_depth;
        }
    }

    /// Goes to a frame as `gotoAndPlay` does: going back rebuilds the timeline's children from
    /// the first frame, going forward applies the frames between; only the target frame's
    /// actions run.
    pub(super) fn goto(&mut self, id: ClipId, target: usize, actions: bool) {
        let n = self.frame_count(id);
        let target = target.min(n - 1);
        let cur = self.clips[id].frame;
        if target == cur {
            // the playhead is already there: the frame is not entered again
            return;
        }
        if target > cur {
            for f in cur + 1..target {
                self.enter_frame(id, f, false);
            }
            self.enter_frame(id, target, actions);
        } else {
            // back: what the timeline placed after the target frame goes, the rest is built
            // again from the first frame (a child placed by the same item stays as it is)
            let later: Vec<i32> = self.clips[id].children.iter().filter(|(d, c)| **d < SCRIPT_DEPTH && self.clips[**c].placed > target).map(|(d, _)| *d).collect();
            for d in later {
                if let Some(c) = self.clips[id].children.remove(&d) {
                    self.kill(c);
                }
            }
            for f in 0..target {
                self.enter_frame(id, f, false);
            }
            self.enter_frame(id, target, actions);
        }
    }

    /// Runs the waiting actions until none are left.
    pub(super) fn run_queue(&mut self) {
        let mut runs = 0u32;
        while let Some(p) = self.queue.pop_front() {
            if !self.clips[p.clip].alive {
                continue;
            }
            runs += 1;
            if self.trace {
                let c = &self.clips[p.clip];
                eprintln!("apt: run frame {} of clip {:?} #{} ({}, char {})", p.frame, c.name, p.clip, c.flash.name, p.ch);
            }
            if runs > 20_000 {
                let c = &self.clips[p.clip];
                let what = format!("actions never end: clip {:?} of {} frame {}", c.name, c.flash.name, p.frame);
                self.note_stub(what);
                self.queue.clear();
                return;
            }
            let flash = p.flash.clone();
            let Some(Item::Action(code)) = Self::frames_of(&flash, p.ch).get(p.frame).and_then(|f| f.items.get(p.item)) else { continue };
            self.depth = 0;
            self.run_top(p.clip, code);
        }
    }

    /// Asks for a movie to be loaded into a clip (`loadMovie`); an empty path unloads the clip.
    pub(super) fn request_load(&mut self, path: &str, target: ClipId) {
        if self.trace {
            eprintln!("apt: loadMovie {path:?} into clip {:?} #{target}", self.clips[target].name);
        }
        if path.is_empty() {
            self.clear(target);
            self.clips[target].props = Props::default();
            return;
        }
        let name = Self::stem(path);
        // a movie the game still has in memory (loaded into another clip) is not loaded again
        let cached = self.clips.iter().any(|c| c.alive && c.loaded.as_deref() == Some(&name));
        if cached && self.flashes.contains_key(&name) {
            self.scheduled.push((self.frame + self.cache_delay, name, target));
            return;
        }
        self.requests.push_back(LoadRequest { name, target });
    }

    /// Fills the clips that asked for a movie the game had in memory, once their time is up
    /// (call at the start of a game frame, after setting `frame`).
    pub fn tick_loads(&mut self) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.scheduled).into_iter().partition(|(at, ..)| *at <= self.frame);
        self.scheduled = later;
        for (_, name, target) in due {
            if self.trace {
                eprintln!("apt: start {name} in clip #{target} at game frame {}", self.frame);
            }
            if self.clips[target].alive
                && let Some(flash) = self.flashes.get(&name).cloned()
            {
                self.fill(target, flash, &name);
            }
        }
        self.run_queue();
    }

    /// A command to the game.
    pub(super) fn send(&mut self, name: &str, v: &Value) {
        // names with a leading underscore are the wrapper's own (its debug messages and its
        // flags); the game's observer shows no command of them
        if name.starts_with('_') {
            return;
        }
        let argument = v.to_text().to_string();
        self.commands.push(Command { frame: self.frame, name: name.to_string(), argument });
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    pub(super) fn pool_value(pool: &[Const], v: &Const) -> Value {
        match v {
            Const::Pool(i) => match pool.get(*i as usize) {
                Some(Const::Str(s)) => Value::Str(s.as_str().into()),
                Some(c) => Self::const_value(c),
                None => Value::Undefined,
            },
            c => Self::const_value(c),
        }
    }

    fn const_value(c: &Const) -> Value {
        match c {
            Const::Str(s) => Value::Str(s.as_str().into()),
            Const::Float(f) => Value::Num(f64::from(*f)),
            Const::Int(i) => Value::Num(f64::from(*i)),
            Const::Bool(b) => Value::Bool(*b),
            Const::Undefined => Value::Undefined,
            Const::Pool(_) | Const::Register(_) | Const::Unknown(..) => Value::Undefined,
        }
    }

    /// The clip a path names, relative to `from` (`_root`, `_parent`, `a.b`, `a/b`).
    pub fn resolve_path(&self, from: ClipId, path: &str) -> Option<ClipId> {
        let mut cur = from;
        let path = if let Some(p) = path.strip_prefix('/') {
            cur = self.root?;
            p
        } else {
            path
        };
        for seg in path.split(['.', '/']).filter(|s| !s.is_empty()) {
            cur = match seg {
                "_root" | "_level0" => self.root?,
                "_parent" => self.clips[cur].parent?,
                "this" => cur,
                s => self.child_named(cur, s)?,
            };
        }
        Some(cur)
    }

    /// What a clip holds, for a probe: its frame and its variables (`path` as a script gives it).
    pub fn describe(&self, path: &str) -> String {
        let Some(root) = self.root else { return "no movie".into() };
        let Some(id) = self.resolve_path(root, path) else { return format!("{path}: no such clip") };
        let c = &self.clips[id];
        fn show(v: &Value, depth: u32) -> String {
            match v {
                Value::Obj(o) if depth > 0 => match &o.borrow().kind {
                    Kind::Plain => format!("{{{}}}", o.borrow().props.keys().map(|k| format!("{k}: {}", o.borrow().props.get(k).map_or(String::new(), |v| show(v, depth - 1)))).collect::<Vec<_>>().join(", ")),
                    _ => v.to_text().to_string(),
                },
                Value::Str(s) => format!("{s:?}"),
                v => v.to_text().to_string(),
            }
        }
        let vars: Vec<String> = c.props.keys().filter(|k| !matches!(c.props.get(k), Some(Value::Obj(o)) if matches!(o.borrow().kind, Kind::Func(_)))).map(|k| format!("{k}={}", c.props.get(k).map_or(String::new(), |v| show(v, 3)))).collect();
        let kids: Vec<String> = c.children.iter().map(|(d, k)| format!("{d}:{}", self.clips[*k].name)).collect();
        format!("{path}: #{id} {} char {} frame {} playing {} visible {} at ({}, {}) vars [{}] children [{}]", c.flash.name, c.ch, c.frame, c.playing, c.visible, c.matrix[4], c.matrix[5], vars.join(", "), kids.join(", "))
    }

    pub fn child_named(&self, id: ClipId, name: &str) -> Option<ClipId> {
        self.clips[id].children.values().copied().find(|&c| self.clips[c].name == name)
    }
}
