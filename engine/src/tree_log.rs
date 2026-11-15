//! A log of the recompiled Bustin' Out's interpreter (its `src/tree_log.cpp`, BUSTIN_TREE_LOG),
//! read for the engine's checks (lockstep, replay).

use std::collections::BTreeMap;

use crate::bhav::Tree;

/// An interpreter at its first step ('k'): the tick, its address, the frame it ran, its first 32
/// words (+8 its stack's first frame) and the 512 bytes below that frame (guest order).
#[derive(Clone, Debug)]
pub struct StackDump {
    pub tick: u32,
    pub sim: u32,
    pub frame: u32,
    pub words: [u32; 32],
    pub below: Vec<u8>,
}

/// A texture the original set up (GXInitTexObj, 'x'): the tick, its image's address, width,
/// height, GX format, wraps, mipmap flag, its first level's size and FNV-1a hash.
#[derive(Clone, Copy, Debug)]
pub struct TexObjRec {
    pub tick: u32,
    pub image: u32,
    pub width: u16,
    pub height: u16,
    pub format: u8,
    pub wrap: (u8, u8),
    pub mip: u8,
    pub size: u32,
    pub hash: u64,
}

/// A palette the original set up (GXInitTlutObj, 'u'): the tick, its address, its GX format
/// (0 IA8, 1 RGB565, 2 RGB5A3) and its entries.
#[derive(Clone, Debug)]
pub struct TlutRec {
    pub tick: u32,
    pub address: u32,
    pub format: u8,
    pub entries: Vec<u16>,
}

/// A strip of short arrays the device played (the command list's opcode 56, 'w'): the tick, the
/// command's first 16 words (the opcode, the count, its arguments) and 32 bytes at each word that
/// points into memory (zeros otherwise).
#[derive(Clone, Debug)]
pub struct StripRec {
    pub tick: u32,
    pub words: [u32; 16],
    pub at: Vec<[u8; 32]>,
}

/// The material a draw was set up with ('m': ERC_SetMaterialStages's device as it was called):
/// the tick, the layer count, the two layers' bytes, the two texture slots (a word and 128 bytes
/// at it each), and how many strip records came before it (the draw it is for comes next).
#[derive(Clone, Debug)]
pub struct MaterialRec {
    pub tick: u32,
    pub layers: u32,
    pub layer: [[u8; 0x18]; 2],
    pub textures: [(u32, [u8; 128]); 2],
    pub strips_before: usize,
}

/// A texture loaded for a draw ('l': GXLoadTexObjPreLoaded): the tick, the map, the GXTexObj's
/// bytes (its image at +0x0C, the physical address >> 5), and how many strip records came before.
#[derive(Clone, Debug)]
pub struct TexLoadRec {
    pub tick: u32,
    pub map: u8,
    pub obj: [u8; 32],
    pub strips_before: usize,
}

impl TexLoadRec {
    /// The image's address, as the CPU sees it.
    pub fn image(&self) -> u32 {
        let w = u32::from_be_bytes([self.obj[12], self.obj[13], self.obj[14], self.obj[15]]);
        ((w & 0x1F_FFFF) << 5) | 0x8000_0000
    }
}

/// A matrix the original loaded ('p': GX position matrices by id, and ERC_ApplyState's view
/// (id 0xF0, when it changes) and world matrix (0xF1, for each model-view made); 'j': a
/// projection, the id its type): the tick, the id, its floats, and how many strip records came
/// before (the draw it is for comes next).
#[derive(Clone, Debug)]
pub struct MatrixRec {
    pub tick: u32,
    pub projection: bool,
    pub id: u8,
    pub values: Vec<f32>,
    pub strips_before: usize,
}

/// An object's lights put at its nodes ('1'): the object, its definition's resources, and each
/// light: its kind and node (the definition's list), the light object, the node's 16 words, the
/// light's position after and its +204 and +208 (as words).
#[derive(Clone, Debug)]
pub struct ObjectLightsRec {
    pub tick: u32,
    pub object: u32,
    pub resources: u32,
    pub lights: Vec<ObjectLight>,
}

#[derive(Clone, Debug)]
pub struct ObjectLight {
    pub kind: i32,
    pub node: u32,
    pub light: u32,
    pub words: [u32; 16],
    pub at: [f32; 3],
    pub radius: f32,
    pub reach: f32,
}

/// A window light's colour filled ('z'): the light's index and its object's room, its position,
/// the sun, the nearest lights listed and what the filler left.
#[derive(Clone, Debug)]
pub struct WindowFillRec {
    pub tick: u32,
    pub light: u32,
    /// The light object's room (+0x34; 0 outside).
    pub room: i32,
    pub at: [f32; 3],
    pub sun: [f32; 3],
    /// The nearest lights listed (up to 4, up to the list's first gap, as the filler reads
    /// them): each one's index, position, reach and colour as the filler read them.
    pub near: Vec<(u32, [f32; 3], f32, [f32; 3])>,
    /// After: the object's mix (+152), its base (+148), the light's reach and colour.
    pub mix: [f32; 3],
    pub base: f32,
    pub reach: f32,
    pub colour: [f32; 3],
}

/// The lights rebuilt ('t'): the room count, the other and own lights' counts (own from 256),
/// each light object (the object, its vtable, its room, its changed flag) and the lights (0 to
/// the other count, then 256 to the own: position, reach, colour as words, then the object).
#[derive(Clone, Debug)]
pub struct LightRebuildRec {
    pub tick: u32,
    pub rooms: u32,
    pub other: u32,
    pub own: u32,
    pub objects: Vec<[u32; 4]>,
    pub lights: Vec<[u32; 8]>,
    /// How many grid builds ('e') came before it (its own build is the last of those).
    pub builds_before: usize,
    /// How many '1' records (objects' lights put at their nodes) came before it.
    pub object_lights_before: usize,
}

/// The light grid's cells built ('e'): the three constants, room ids 0-15's light ranges (the
/// other lights' first and end, the room's own first and end), the lights (position, reach) and
/// cells x, y 2..37 (x outer, 9 words each: 4 room ids, then 4 light indices for each).
#[derive(Clone, Debug)]
pub struct GridBuildRec {
    pub tick: u32,
    pub constants: [f32; 3],
    pub ranges: [[u32; 4]; 16],
    pub lights: Vec<([f32; 3], f32)>,
    pub cells: Vec<[u32; 9]>,
    /// How many 'f' records (rooms added to cells) came before it.
    pub adds_before: usize,
}

/// A light ranking ('d'): the point, the lights ranked (first..end, each one's position and
/// reach as words), the list's words (two a slot) before and after, and the two constants.
#[derive(Clone, Debug)]
pub struct LightRankRec {
    pub tick: u32,
    pub point: [f32; 3],
    pub first: u32,
    pub lights: Vec<[u32; 4]>,
    pub before: Vec<u32>,
    pub after: Vec<u32>,
    pub constants: [f32; 2],
}

/// A room's ambient and light triples ('a'): the tick, the room id, its 80-byte light record (as
/// words), the triples and the constant they fall back to.
#[derive(Clone, Copy, Debug)]
pub struct RoomTriplesRec {
    pub tick: u32,
    pub id: u32,
    pub record: [u32; 20],
    pub ambient: [f32; 3],
    pub light: [f32; 3],
    pub fallback: f32,
}

/// A floor group's colour base and scale ('b'), with what they were made from: the material's two
/// triples (+0x44, +0x54), the room's two and the constant.
#[derive(Clone, Copy, Debug)]
pub struct GroupScaleRec {
    pub tick: u32,
    pub group: u32,
    pub base: [f32; 3],
    pub scale: [f32; 3],
    pub material: [[f32; 3]; 2],
    pub room: [[f32; 3]; 2],
    pub k: f32,
}

/// A room's floor relit ('n'): the tick, the room's id, the light grid record just before, and
/// each group's entries: the group, then each entry's word (its row, its first and last x with
/// the half tiles' flags in their top two bits) and its vertices' colours.
#[derive(Clone, Debug)]
pub struct RoomFloorRec {
    pub tick: u32,
    pub id: u32,
    pub grid: Option<usize>,
    /// How many group scale records ('b') came before it: its groups' are the last of those,
    /// one a group in order.
    pub scales_before: usize,
    pub groups: Vec<(u32, Vec<FloorEntry>)>,
}

/// A floor entry: its word (row, first and last x with the half tiles' flags) and its colours.
pub type FloorEntry = (u32, Vec<u32>);

/// A LightGrid_Sum call ('i'): the tick, the room id, and each lit point: its x and row, its
/// cell's lights (each the original's 32 bytes as words) and the summed colour.
#[derive(Clone, Debug)]
pub struct LightGridRec {
    pub tick: u32,
    pub id: u32,
    pub points: Vec<LitPoint>,
}

/// A lit point: its x, its row, its cell's lights (32 bytes each as words) and its summed colour.
pub type LitPoint = (u32, u32, Vec<[u32; 8]>, [f32; 3]);

/// A logged strip vertex: its position (x, y, z, w), UV and colour.
pub type StripVertex = ([f32; 4], [f32; 2], u32);

/// 'c': the lot camera after its step a frame (logged when it changed): the wanted parameters
/// (target x y z, distance, rotation, tilt), the live ones, the easing mode, the eye, the followed
/// cursor's address and the camera's.
#[derive(Clone, Debug)]
pub struct CamRec {
    pub tick: u32,
    pub wanted: [f32; 6],
    pub live: [f32; 6],
    pub mode: u32,
    pub eye: [f32; 3],
    pub cursor: u32,
    pub camera: u32,
    /// The frame's time step (seconds) the step eased by.
    pub dt: f32,
    /// How many steps ran since the last record (this one's included).
    pub steps: u32,
    /// Which step logged it: 0 the frame's (ESimsCam_Ease), 1 SetWinPos (which eases once more).
    pub which: u32,
    /// The cursor's place (0 without a cursor).
    pub cursor_at: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct OrientRec {
    pub tick: u32,
    pub model: u32,
    pub m: [f32; 16],
}

/// An object whose resources the original's data manager collected ('s': collectResInfoForSel):
/// the tick, the selector's address, its material's and its extra data's resource ids, its
/// definition's kind, and the selector's and definition's bytes.
#[derive(Clone, Debug)]
pub struct SelectorRec {
    pub tick: u32,
    pub selector: u32,
    pub material_id: u32,
    pub kind: u32,
    pub extra_id: u32,
    pub bytes: Vec<u8>,
    pub definition: Vec<u8>,
}

/// A tree as the original had it loaded: its nodes' address, id, file, and its nodes' raw bytes.
#[derive(Clone, Debug, Default)]
pub struct TreeRec {
    pub nodes: u32,
    pub tree: u16,
    pub file: u32,
    pub raw: Vec<u8>,
}

/// The original's expression during a step: what its variable lookup found for each side.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExprRec {
    pub lhs_scope: i16,
    pub lhs_data: i16,
    pub rhs_scope: i16,
    pub rhs_data: i16,
    pub op: u8,
    /// 1 a 16-bit pointer on the left, 2 a float one, 4/8 a lookup failed.
    pub flags: u8,
    pub rhs: i16,
    pub lhs: i16,
    pub lhs_after: i16,
    pub lhs_float: u32,
    pub lhs_float_after: u32,
    pub result: u32,
}

/// A variable lookup of the original's (any primitive's).
#[derive(Clone, Copy, Debug, Default)]
pub struct LookupRec {
    pub object: u32,
    pub caller: u32,
    pub scope: i16,
    pub data: i16,
    pub value: i16,
    /// 1 found, 2 a 16-bit pointer, 4 a float one.
    pub flags: u8,
    pub ptr: u32,
    pub fptr: u32,
    pub fvalue: u32,
}

#[derive(Clone, Debug, Default)]
pub struct StepRec {
    pub sim: u32,
    pub frame: u32,
    pub file: u32,
    pub nodes: u32,
    pub tree: u16,
    pub node: u16,
    pub after: u16,
    pub error: u16,
    pub look: u8,
    pub result: u8,
    pub params: Vec<i16>,
    pub locals: Vec<i16>,
    pub params_after: Vec<i16>,
    pub locals_after: Vec<i16>,
    /// The interpreter's own parameters (its +44), for calls giving none.
    pub defaults: [i16; 8],
    /// The frame's (+4), before and after.
    pub stack_object: i16,
    pub stack_object_after: i16,
    /// The tree GetNodeRef found, as loaded at the time: its place in `Log::trees`.
    pub t: Option<usize>,
    pub expr: Option<ExprRec>,
    /// Made during the step, in order.
    pub lookups: Vec<LookupRec>,
    pub tick: u32,
    /// A turn in the tick's pass (logs with 'Q' records).
    pub in_pass: bool,
}

/// The original's object list, when it changed: each object's address and id.
#[derive(Clone, Debug, Default)]
pub struct ListRec {
    pub tick: u32,
    pub objects: Vec<u32>,
    pub ids: Vec<i16>,
}

/// The tick's pass over the objects: the id its cursor goes on from (0: a new pass), and the id
/// it stopped at (0: it went through).
#[derive(Clone, Copy, Debug, Default)]
pub struct PassRec {
    pub from: u32,
    pub stopped_at: u32,
}

/// An object's place when it changed, at the start of a tick (the 'P' record): sixteenths of a
/// tile, and its level; -16, -16 off the lot.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlaceRec {
    pub tick: u32,
    pub id: i16,
    pub x: i32,
    pub y: i32,
    pub level: i32,
}

/// An autonomy score's call (the 'A' record): the motives it read, the adverts (16 of
/// personality, minimum, delta; big-endian in the record), its score.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScoreRec {
    pub tick: u32,
    pub score: f32,
    pub motives: [f32; 16],
    pub adverts: [[i16; 3]; 16],
}

/// A base score ('B').
#[derive(Clone, Copy, Debug, Default)]
pub struct BaseRec {
    pub tick: u32,
    pub score: f32,
}

/// An entry of autonomy's sorted list.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChoiceEntry {
    pub id: i16,
    pub interaction: u16,
    pub value: f32,
    pub takes_best: u32,
}

/// Autonomy's sorted list ('Y': the sort is recursive, so a tick's last is the whole list).
#[derive(Clone, Debug, Default)]
pub struct ChoiceListRec {
    pub tick: u32,
    pub entries: Vec<ChoiceEntry>,
}

/// A route the original planned (the 'G' record, Route_Plan): its waypoints, sixteenths of a
/// tile.
#[derive(Clone, Debug, Default)]
pub struct PlanRec {
    pub tick: u32,
    pub state: u32,
    pub points: Vec<(i32, i32)>,
    /// The request's goals (the 'H' record before it, in newer logs): 16-byte entries, raw
    /// words; the first two are the place in sixteenths.
    pub goals: Vec<[u32; 4]>,
    /// The goal it took (the request's +88), -1 for none or an older log.
    pub chosen: i32,
    /// The obstacles the search gathered from the objects (the 'X' record, in newer logs):
    /// {x0, y0, x1, y1, cost} in sixteenths (the record's {y0, x0, y1, x1} turned); and the global width it grows them by half of.
    pub obstacles: Vec<[i32; 5]>,
    pub obstacle_width: i32,
    /// The objects the search's loop asked about (the 'J' record, in newer logs), in order: the
    /// id and whether it was left out.
    /// (the listed object's id, left out, the part's offset from it)
    pub tested: Vec<(i16, bool, i32)>,
    /// The search's own rectangles ('D', in newer logs): the lot's border, the house's blocks
    /// and walls, and the paved regions at cost 0; {x0, y0, x1, y1, cost}.
    pub regions: Vec<[i32; 5]>,
    /// The planner's search's coarse way through its map's nodes ('I', in newer logs), the last
    /// before the plan: (x, y) in sixteenths.
    pub coarse: Vec<(i32, i32)>,
    /// The two rect lists the search's region lookup walked ('r', after that 'I', in newer
    /// logs): the graph's +4 vector and the one behind its +8 (empty when null); {y0, x0, y1,
    /// x1, cost}, raw (turned).
    pub lookup_regions: [Vec<[i32; 5]>; 2],
    /// The request's grower settings at that search ('r'): +32 the aspect trim's switch, +36
    /// its ratio, +40 the cell cap's switch, +44 the cap, +48 the cost of a cell in no region.
    pub grower: [u32; 5],
    /// What the plan read for its point after the goal ('h', after 'H', in newer logs): the
    /// request's +72 (the kind: -3 a step the routing object's way, -1/-2 towards an object),
    /// its +48, the routing object's facing, and the target point it steps towards (+200, +204
    /// of the request's +80 object as its virtual moves it), then the object whose direction the
    /// other kinds step by: its id and its +144 (logged as its data 1, but 0 throughout), and the
    /// planning object's id (-1 none).
    pub approach: Option<[i32; 8]>,
}

/// A walker's step ('Z', Route_Follow 800AE0A4): its follower and its state before and after.
#[derive(Clone, Copy, Debug, Default)]
pub struct FollowRec {
    pub tick: u32,
    pub follower: u32,
    pub before: FollowState,
    pub after: FollowState,
}

/// A follower's state (+28: 1 starting, 2 walking, 3 arrived, ...), its place (sixteenths, as
/// floats) and its speed factor (+124: 1, or 5 for the fast mode).
#[derive(Clone, Copy, Debug, Default)]
pub struct FollowState {
    pub state: u32,
    pub x: f32,
    pub y: f32,
    pub factor: f32,
    /// The point state 5 moves to (+116/+120, stored turned) and +132 (1: its reach 9, else 3).
    pub point: (f32, f32),
    pub mode: u32,
}

/// A random number the game drew (the 'R' record): site 0 and 1 autonomy's choice (80226DD4,
/// 80226DE8), 2 a slot goal's score (8024F030), 3 the slot builder's pick (8024F504).
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomRec {
    pub tick: u32,
    pub site: u8,
    pub value: u32,
}

/// A person's 16 motives when they changed, at the start of a tick (the 'N' record).
#[derive(Clone, Copy, Debug, Default)]
pub struct MotiveRec {
    pub tick: u32,
    pub id: i16,
    pub motives: [f32; 16],
}

#[derive(Clone, Debug, Default)]
pub struct Log {
    /// Each interpreter at its first step ('k', in newer logs): the tick, its address, the frame it
    /// ran, its first 32 words and the 512 bytes below that frame (guest order).
    pub stack_dumps: Vec<StackDump>,
    /// The textures the original set up ('x', in newer logs).
    pub textures: Vec<TexObjRec>,
    /// The palettes the original set up ('u', in newer logs).
    pub tluts: Vec<TlutRec>,
    /// The objects whose resources were collected ('s', in newer logs, at most 2,000).
    pub selectors: Vec<SelectorRec>,
    /// The static models' orientations ('o'), in newer logs.
    pub orients: Vec<OrientRec>,
    pub cams: Vec<CamRec>,
    /// The matrices loaded ('p', at most 20,000) and projections ('j', at most 2,000), in newer logs.
    pub matrices: Vec<MatrixRec>,
    /// The textures loaded for draws ('l', in newer logs, at most 16,000).
    pub tex_loads: Vec<TexLoadRec>,
    /// The materials draws were set up with ('m', in newer logs, at most 8,000).
    pub materials: Vec<MaterialRec>,
    /// The strips of short arrays the device played ('w', in newer logs, at most 4,000).
    pub strips: Vec<StripRec>,
    /// The list's other draws ('g', in newer logs: opcodes other than 56 and 66), each with how
    /// many 'w' strips and how many texture loads came before it.
    pub other_draws: Vec<(StripRec, usize, usize)>,
    /// 'v': a floor strip's (op 42's) whole vertices, after its 'g' record: that record's index
    /// in `other_draws`, and each vertex's position (x, y, z, w), UV and colour.
    pub strip_vertices: Vec<(usize, Vec<StripVertex>)>,
    /// 'i': each LightGrid_Sum call's lit points.
    pub light_grids: Vec<LightGridRec>,
    /// 'b': floor groups' colour bases and scales (tick, the word Light_GroupScale was given less
    /// 4, base, scale), one a group as its room's floor is relit.
    pub group_scales: Vec<GroupScaleRec>,
    /// 'a': rooms' ambient and light triples, with the rooms' records they come from.
    pub room_triples: Vec<RoomTriplesRec>,
    /// 'd': light rankings.
    pub light_ranks: Vec<LightRankRec>,
    /// 'e': the light grid's cells built.
    pub grid_builds: Vec<GridBuildRec>,
    /// 'f': rooms added to light grid cells (tick, x, y, room id, caller).
    pub cell_adds: Vec<[u32; 5]>,
    /// 't': the lights rebuilt.
    pub light_rebuilds: Vec<LightRebuildRec>,
    /// 'y': the window light objects (the object, side, reach, two colour fields, the game
    /// object, its kind (+0xC), then its first 64 words and its kind's key, +0x138).
    pub window_lights: Vec<(Vec<u32>, Vec<u32>)>,
    /// '1': objects' lights put at their nodes.
    pub object_lights: Vec<ObjectLightsRec>,
    /// 'z': the window lights' colours filled.
    pub window_fills: Vec<WindowFillRec>,
    /// 'n': rooms' floors relit, each after the 'i' record of its own light grid (its index there
    /// is `light_grids.len()` less one when read).
    pub room_floors: Vec<RoomFloorRec>,
    pub trees: Vec<TreeRec>,
    pub lists: Vec<ListRec>,
    /// Tick -> its pass over the objects (logs with 'Q' records).
    pub passes: BTreeMap<u32, PassRec>,
    pub steps: Vec<StepRec>,
    pub places: Vec<PlaceRec>,
    pub motives: Vec<MotiveRec>,
    pub scores: Vec<ScoreRec>,
    pub bases: Vec<BaseRec>,
    pub choice_lists: Vec<ChoiceListRec>,
    pub randoms: Vec<RandomRec>,
    pub plans: Vec<PlanRec>,
    pub follows: Vec<FollowRec>,
    pub ticks: u32,
}

impl Log {
    /// The tree record a step names.
    pub fn tree_of(&self, s: &StepRec) -> Option<&TreeRec> {
        s.t.map(|t| &self.trees[t])
    }
}

struct Bytes<'a> {
    b: &'a [u8],
    p: usize,
}

impl Bytes<'_> {
    fn has(&self, n: usize) -> bool {
        self.p + n <= self.b.len()
    }
    fn u8(&mut self) -> u8 {
        self.p += 1;
        self.b[self.p - 1]
    }
    fn u16(&mut self) -> u16 {
        self.p += 2;
        u16::from_le_bytes([self.b[self.p - 2], self.b[self.p - 1]])
    }
    fn i16(&mut self) -> i16 {
        self.u16() as i16
    }
    fn u32(&mut self) -> u32 {
        self.p += 4;
        u32::from_le_bytes(self.b[self.p - 4..self.p].try_into().unwrap())
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
}

/// The log at `path`; `None` if it cannot be read or has a record of a kind not known (which
/// is reported, as the C++ reader did).
pub fn read_log(path: &str) -> Option<Log> {
    let b = std::fs::read(path).ok()?;
    let mut r = Bytes { b: &b, p: 0 };
    let mut log = Log::default();
    let mut request: Option<(i32, Vec<[u32; 4]>)> = None;
    let mut obstacles: Option<(i32, Vec<[i32; 5]>)> = None;
    let mut tested: Option<Vec<(i16, bool, i32)>> = None;
    let mut regions: Option<Vec<[i32; 5]>> = None;
    let mut coarse: Option<Vec<(i32, i32)>> = None;
    let mut lookup_regions: Option<[Vec<[i32; 5]>; 2]> = None;
    let mut grower: Option<[u32; 5]> = None;
    let mut approach: Option<[i32; 8]> = None;
    let mut at: BTreeMap<u32, usize> = BTreeMap::new();
    let mut pending: Option<ExprRec> = None;
    let mut pending_lookups: Vec<LookupRec> = Vec::new();
    let mut in_pass = false;
    while r.p < b.len() {
        let k = r.u8();
        match k {
            b'K' => {
                if !r.has(4) {
                    break;
                }
                r.u32();
                log.ticks += 1;
            }
            b'w' => {
                if !r.has(4 + 16 * 36) {
                    break;
                }
                let tick = r.u32();
                let mut words = [0u32; 16];
                let mut at = Vec::with_capacity(16);
                for w in &mut words {
                    *w = r.u32();
                    let mut b = [0u8; 32];
                    b.copy_from_slice(&r.b[r.p..r.p + 32]);
                    r.p += 32;
                    at.push(b);
                }
                log.strips.push(StripRec { tick, words, at });
            }
            b'g' => {
                if !r.has(4 + 16 * 36) {
                    break;
                }
                let tick = r.u32();
                let mut words = [0u32; 16];
                let mut at = Vec::with_capacity(16);
                for w in &mut words {
                    *w = r.u32();
                    let mut b = [0u8; 32];
                    b.copy_from_slice(&r.b[r.p..r.p + 32]);
                    r.p += 32;
                    at.push(b);
                }
                let (before, loads) = (log.strips.len(), log.tex_loads.len());
                log.other_draws.push((StripRec { tick, words, at }, before, loads));
            }
            b'i' => {
                if !r.has(12) {
                    break;
                }
                let (tick, id, spans) = (r.u32(), r.u32(), r.u32());
                let mut points = Vec::new();
                let mut ok = true;
                'spans: for _ in 0..spans {
                    if !r.has(12) {
                        ok = false;
                        break;
                    }
                    let (row, first, last) = (r.u32(), r.u32(), r.u32());
                    for x in first..=last {
                        if !r.has(8) {
                            ok = false;
                            break 'spans;
                        }
                        let idx: Vec<i16> = (0..4).map(|_| { let v = i16::from_le_bytes([r.b[r.p], r.b[r.p + 1]]); r.p += 2; v }).collect();
                        let n = idx.iter().take_while(|&&i| i != -1).count();
                        if !r.has(n * 32 + 12) {
                            ok = false;
                            break 'spans;
                        }
                        let lights = (0..n).map(|_| [0; 8].map(|_| r.u32())).collect();
                        let out = [0; 3].map(|_| f32::from_bits(r.u32()));
                        points.push((x, row, lights, out));
                    }
                }
                if !ok {
                    break;
                }
                log.light_grids.push(LightGridRec { tick, id, points });
            }
            b'b' => {
                if !r.has(84) {
                    break;
                }
                let (tick, group) = (r.u32(), r.u32());
                let mut three = || [0; 3].map(|_| f32::from_bits(r.u32()));
                let (base, scale) = (three(), three());
                let material = [three(), three()];
                let room = [three(), three()];
                let k = f32::from_bits(r.u32());
                log.group_scales.push(GroupScaleRec { tick, group, base, scale, material, room, k });
            }
            b'1' => {
                if !r.has(16) {
                    break;
                }
                let (tick, object, resources, count) = (r.u32(), r.u32(), r.u32(), r.u32() as usize);
                if count > 8 || !r.has(count * 4 * 24) {
                    break;
                }
                let f = |v: u32| f32::from_bits(v);
                let lights = (0..count)
                    .map(|_| {
                        let (kind, node, light) = (r.u32() as i32, r.u32(), r.u32());
                        let mut words = [0; 16];
                        for w in &mut words {
                            *w = r.u32();
                        }
                        let at = [f(r.u32()), f(r.u32()), f(r.u32())];
                        let (radius, reach) = (f(r.u32()), f(r.u32()));
                        ObjectLight { kind, node, light, words, at, radius, reach }
                    })
                    .collect();
                log.object_lights.push(ObjectLightsRec { tick, object, resources, lights });
            }
            b'z' => {
                if !r.has(4 * 53) {
                    break;
                }
                let f = |v: u32| f32::from_bits(v);
                let (tick, light, room) = (r.u32(), r.u32(), r.u32() as i32);
                let mut v3 = || [f(r.u32()), f(r.u32()), f(r.u32())];
                let (at, sun) = (v3(), v3());
                let list: Vec<u32> = (0..4).map(|_| r.u32()).collect();
                let mut near = Vec::new();
                let mut gap = false;
                for &p in &list {
                    let w: Vec<u32> = (0..8).map(|_| r.u32()).collect();
                    gap |= p == 0;
                    if !gap {
                        near.push((w[0], [f(w[1]), f(w[2]), f(w[3])], f(w[4]), [f(w[5]), f(w[6]), f(w[7])]));
                    }
                }
                let mix = [f(r.u32()), f(r.u32()), f(r.u32())];
                let base = f(r.u32());
                let reach = f(r.u32());
                let colour = [f(r.u32()), f(r.u32()), f(r.u32())];
                log.window_fills.push(WindowFillRec { tick, light, room, at, sun, near, mix, base, reach, colour });
            }
            b'y' => {
                if !r.has(4) {
                    break;
                }
                let count = r.u32() as usize;
                if !r.has(count * 4 * (7 + 65)) {
                    break;
                }
                for _ in 0..count {
                    let head = (0..7).map(|_| r.u32()).collect();
                    let words = (0..65).map(|_| r.u32()).collect();
                    log.window_lights.push((head, words));
                }
            }
            b't' => {
                if !r.has(20) {
                    break;
                }
                let (tick, rooms, other, own, count) = (r.u32(), r.u32(), r.u32(), r.u32(), r.u32() as usize);
                let lights_n = other as usize + own.saturating_sub(256) as usize;
                if !r.has(16 * count + 32 * lights_n) {
                    break;
                }
                let objects = (0..count).map(|_| [0; 4].map(|_| r.u32())).collect();
                let lights = (0..lights_n).map(|_| [0; 8].map(|_| r.u32())).collect();
                log.light_rebuilds.push(LightRebuildRec { tick, rooms, other, own, objects, lights, builds_before: log.grid_builds.len(), object_lights_before: log.object_lights.len() });
            }
            b'f' => {
                if !r.has(20) {
                    break;
                }
                log.cell_adds.push([0; 5].map(|_| r.u32()));
            }
            b'e' => {
                if !r.has(4 * (4 + 64 + 1)) {
                    break;
                }
                let tick = r.u32();
                let constants = [0; 3].map(|_| f32::from_bits(r.u32()));
                let ranges = [0; 16].map(|_| [0; 4].map(|_| r.u32()));
                let count = r.u32() as usize;
                if !r.has(16 * count + 36 * 36 * 36) {
                    break;
                }
                let lights = (0..count)
                    .map(|_| {
                        let w = [0; 4].map(|_| f32::from_bits(r.u32()));
                        ([w[0], w[1], w[2]], w[3])
                    })
                    .collect();
                let cells = (0..36 * 36).map(|_| [0; 9].map(|_| r.u32())).collect();
                log.grid_builds.push(GridBuildRec { tick, constants, ranges, lights, cells, adds_before: log.cell_adds.len() });
            }
            b'd' => {
                if !r.has(24) {
                    break;
                }
                let tick = r.u32();
                let point = [0; 3].map(|_| f32::from_bits(r.u32()));
                let (first, end, slots) = (r.u32(), r.u32(), r.u32() as usize);
                let count = end.saturating_sub(first) as usize;
                if !r.has(16 * count + 16 * slots + 8) {
                    break;
                }
                let lights = (0..count).map(|_| [0; 4].map(|_| r.u32())).collect();
                let before = (0..2 * slots).map(|_| r.u32()).collect();
                let after = (0..2 * slots).map(|_| r.u32()).collect();
                let constants = [0; 2].map(|_| f32::from_bits(r.u32()));
                log.light_ranks.push(LightRankRec { tick, point, first, lights, before, after, constants });
            }
            b'a' => {
                if !r.has(4 * 29) {
                    break;
                }
                let (tick, id) = (r.u32(), r.u32());
                let record = [0; 20].map(|_| r.u32());
                let mut three = || [0; 3].map(|_| f32::from_bits(r.u32()));
                let (ambient, light) = (three(), three());
                let fallback = f32::from_bits(r.u32());
                log.room_triples.push(RoomTriplesRec { tick, id, record, ambient, light, fallback });
            }
            b'n' => {
                if !r.has(12) {
                    break;
                }
                let (tick, _room, id) = (r.u32(), r.u32(), r.u32());
                let mut groups = Vec::new();
                let mut ok = true;
                loop {
                    if !r.has(4) {
                        ok = false;
                        break;
                    }
                    let g = r.u32();
                    if g == 0 {
                        break;
                    }
                    if !r.has(4) {
                        ok = false;
                        break;
                    }
                    let count = r.u32();
                    let mut entries = Vec::new();
                    for _ in 0..count {
                        if !r.has(8) {
                            ok = false;
                            break;
                        }
                        let (word, verts) = (r.u32(), r.u32() as usize);
                        if !r.has(4 * verts) {
                            ok = false;
                            break;
                        }
                        entries.push((word, (0..verts).map(|_| r.u32()).collect()));
                    }
                    if !ok {
                        break;
                    }
                    groups.push((g, entries));
                }
                if !ok {
                    break;
                }
                log.room_floors.push(RoomFloorRec { tick, id, grid: log.light_grids.len().checked_sub(1), scales_before: log.group_scales.len(), groups });
            }
            b'v' => {
                if !r.has(8) {
                    break;
                }
                let (_tick, n) = (r.u32(), r.u32() as usize);
                if !r.has(n * 28) {
                    break;
                }
                let v = (0..n)
                    .map(|_| {
                        let p = [0; 4].map(|_| f32::from_bits(r.u32()));
                        let uv = [0; 2].map(|_| f32::from_bits(r.u32()));
                        (p, uv, r.u32())
                    })
                    .collect();
                log.strip_vertices.push((log.other_draws.len().saturating_sub(1), v));
            }
            b'c' => {
                if !r.has(4 * 25) {
                    break;
                }
                let tick = r.u32();
                let mut f = [0.0f32; 12];
                for v in &mut f {
                    *v = f32::from_bits(r.u32());
                }
                let mode = r.u32();
                let eye = [0; 3].map(|_| f32::from_bits(r.u32()));
                let (cursor, camera, dt, steps) = (r.u32(), r.u32(), f32::from_bits(r.u32()), r.u32());
                let which = r.u32();
                let cursor_at = [0; 3].map(|_| f32::from_bits(r.u32()));
                let (mut wanted, mut live) = ([0.0; 6], [0.0; 6]);
                wanted.copy_from_slice(&f[..6]);
                live.copy_from_slice(&f[6..]);
                log.cams.push(CamRec { tick, wanted, live, mode, eye, cursor, camera, dt, steps, which, cursor_at });
            }
            b'o' => {
                if !r.has(4 * 18) {
                    break;
                }
                let (tick, model) = (r.u32(), r.u32());
                let mut m = [0.0; 16];
                for v in &mut m {
                    *v = f32::from_bits(r.u32());
                }
                log.orients.push(OrientRec { tick, model, m });
            }
            b's' => {
                if !r.has(5 * 4 + 0x80 + 0xC4) {
                    break;
                }
                let (tick, selector, material_id, kind, extra_id) = (r.u32(), r.u32(), r.u32(), r.u32(), r.u32());
                let bytes = r.b[r.p..r.p + 0x80].to_vec();
                r.p += 0x80;
                let definition = r.b[r.p..r.p + 0xC4].to_vec();
                r.p += 0xC4;
                log.selectors.push(SelectorRec { tick, selector, material_id, kind, extra_id, bytes, definition });
            }
            b'p' | b'j' => {
                if !r.has(4 + 2) {
                    break;
                }
                let (tick, id, n) = (r.u32(), r.u8(), r.u8() as usize);
                if !r.has(4 * n) {
                    break;
                }
                let values = (0..n).map(|_| f32::from_bits(r.u32())).collect();
                log.matrices.push(MatrixRec { tick, projection: k == b'j', id, values, strips_before: log.strips.len() });
            }
            b'l' => {
                if !r.has(4 + 1 + 32) {
                    break;
                }
                let tick = r.u32();
                let map = r.u8();
                let mut obj = [0u8; 32];
                obj.copy_from_slice(&r.b[r.p..r.p + 32]);
                r.p += 32;
                log.tex_loads.push(TexLoadRec { tick, map, obj, strips_before: log.strips.len() });
            }
            b'm' => {
                if !r.has(4 + 4 + 48 + 2 * 132) {
                    break;
                }
                let tick = r.u32();
                let layers = r.u32();
                let mut layer = [[0u8; 0x18]; 2];
                for l in &mut layer {
                    l.copy_from_slice(&r.b[r.p..r.p + 0x18]);
                    r.p += 0x18;
                }
                let mut textures = [(0u32, [0u8; 128]); 2];
                for t in &mut textures {
                    t.0 = r.u32();
                    t.1.copy_from_slice(&r.b[r.p..r.p + 128]);
                    r.p += 128;
                }
                log.materials.push(MaterialRec { tick, layers, layer, textures, strips_before: log.strips.len() });
            }
            b'u' => {
                if !r.has(4 + 4 + 1 + 2) {
                    break;
                }
                let (tick, address, format) = (r.u32(), r.u32(), r.u8());
                let n = r.u16() as usize;
                if !r.has(2 * n) {
                    break;
                }
                let entries = (0..n).map(|_| r.u16()).collect();
                log.tluts.push(TlutRec { tick, address, format, entries });
            }
            b'x' => {
                if !r.has(4 + 4 + 2 + 2 + 4 + 4 + 8) {
                    break;
                }
                let (tick, image) = (r.u32(), r.u32());
                let (width, height) = (r.u16(), r.u16());
                let (format, ws, wt, mip) = (r.u8(), r.u8(), r.u8(), r.u8());
                let size = r.u32();
                let hash = r.u32() as u64 | (r.u32() as u64) << 32;
                log.textures.push(TexObjRec { tick, image, width, height, format, wrap: (ws, wt), mip, size, hash });
            }
            b'k' => {
                if !r.has(12 + 128 + 512) {
                    break;
                }
                let (tick, sim, frame) = (r.u32(), r.u32(), r.u32());
                let words: [u32; 32] = std::array::from_fn(|_| r.u32());
                let below = r.b[r.p..r.p + 512].to_vec();
                r.p += 512;
                log.stack_dumps.push(StackDump { tick, sim, frame, words, below });
            }
            b'L' => {
                // the object list, when it changed
                if !r.has(6) {
                    break;
                }
                let tick = r.u32();
                let cnt = r.u16() as usize;
                if !r.has(6 * cnt) {
                    break;
                }
                let mut l = ListRec { tick, ..Default::default() };
                for _ in 0..cnt {
                    l.objects.push(r.u32());
                    l.ids.push(r.i16());
                }
                log.lists.push(l);
            }
            b'Q' => {
                // the pass over the objects starts: its cursor
                if !r.has(4) {
                    break;
                }
                log.passes.entry(log.ticks).or_default().from = r.u32();
                in_pass = true;
            }
            b'q' => {
                // the pass returns: 0, or the id it stopped at
                if !r.has(4) {
                    break;
                }
                log.passes.entry(log.ticks).or_default().stopped_at = r.u32();
                in_pass = false;
            }
            b'B' => {
                if !r.has(8) {
                    break;
                }
                r.u32();
                log.bases.push(BaseRec { tick: log.ticks, score: r.f32() });
            }
            b'Y' => {
                // autonomy's sorted list
                if !r.has(2) {
                    break;
                }
                let cnt = r.u16() as usize;
                if !r.has(16 * cnt) {
                    break;
                }
                let mut l = ChoiceListRec { tick: log.ticks, entries: Vec::with_capacity(cnt) };
                for _ in 0..cnt {
                    let id = r.i16();
                    r.u16();
                    let interaction = (r.u32() & 0xFFFF) as u16;
                    let value = r.f32();
                    l.entries.push(ChoiceEntry { id, interaction, value, takes_best: r.u32() });
                }
                log.choice_lists.push(l);
            }
            b'G' => {
                // a route planned
                if !r.has(10) {
                    break;
                }
                let state = r.u32();
                r.u32();
                let cnt = r.u16() as usize;
                if !r.has(8 * cnt) {
                    break;
                }
                let points = (0..cnt).map(|_| (r.u32() as i32, r.u32() as i32)).collect();
                let (chosen, goals) = request.take().unwrap_or((-1, Vec::new()));
                let (obstacle_width, obstacles) = obstacles.take().unwrap_or_default();
                let tested = tested.take().unwrap_or_default();
                let regions = regions.take().unwrap_or_default();
                let coarse = coarse.take().unwrap_or_default();
                let lookup_regions = lookup_regions.take().unwrap_or_default();
                let grower = grower.take().unwrap_or_default();
                let approach = approach.take();
                log.plans.push(PlanRec { tick: log.ticks, state, points, goals, chosen, obstacles, obstacle_width, tested, regions, coarse, lookup_regions, grower, approach });
            }
            b'I' => {
                // the planner's search's coarse way (stored turned: y, x)
                if !r.has(2) {
                    break;
                }
                let cnt = r.u16() as usize;
                if !r.has(8 * cnt) {
                    break;
                }
                coarse = Some((0..cnt).map(|_| {
                    let y = r.u32() as i32;
                    (r.u32() as i32, y)
                }).collect());
            }
            b'h' => {
                // what the plan read for its point after the goal
                if !r.has(32) {
                    break;
                }
                approach = Some(std::array::from_fn(|_| r.u32() as i32));
            }
            b'r' => {
                // the search's region lookup's two lists
                let mut lists: [Vec<[i32; 5]>; 2] = Default::default();
                for list in &mut lists {
                    if !r.has(2) {
                        break;
                    }
                    let cnt = r.u16() as usize;
                    if !r.has(20 * cnt) {
                        break;
                    }
                    *list = (0..cnt).map(|_| [r.u32() as i32, r.u32() as i32, r.u32() as i32, r.u32() as i32, r.u32() as i32]).collect();
                }
                lookup_regions = Some(lists);
                if !r.has(20) {
                    break;
                }
                grower = Some([r.u32(), r.u32(), r.u32(), r.u32(), r.u32()]);
            }
            b'Z' => {
                // a walker's step (Route_Follow)
                if !r.has(70) {
                    break;
                }
                let follower = r.u32();
                r.i16();
                let mut w = [[0u32; 8]; 2];
                for half in &mut w {
                    for x in half.iter_mut() {
                        *x = r.u32();
                    }
                }
                let side = |h: &[u32; 8]| FollowState {
                    state: h[0],
                    // stored turned (y, x)
                    x: f32::from_bits(h[2]),
                    y: f32::from_bits(h[1]),
                    factor: f32::from_bits(h[5]),
                    point: (f32::from_bits(h[4]), f32::from_bits(h[3])),
                    mode: h[6],
                };
                log.follows.push(FollowRec { tick: log.ticks, follower, before: side(&w[0]), after: side(&w[1]) });
            }
            b'F' => {
                // a person moved to a place (801F0410), with its callers: read past for now
                if !r.has(38) {
                    break;
                }
                r.i16();
                for _ in 0..9 {
                    r.u32();
                }
            }
            b'J' => {
                // the objects the loop of the plan that follows tested: the part's address, the
                // id of the listed object it belongs to, its offset, left out, 24 words of it
                if !r.has(2) {
                    break;
                }
                let cnt = r.u16() as usize;
                if !r.has(107 * cnt) {
                    break;
                }
                let mut t = Vec::with_capacity(cnt);
                for _ in 0..cnt {
                    r.u32();
                    let id = r.i16();
                    let offset = r.u32() as i32;
                    let out = r.u8() != 0;
                    for _ in 0..24 {
                        r.u32();
                    }
                    t.push((id, out, offset));
                }
                tested = Some(t);
            }
            b'D' => {
                // the search's own rectangles of the plan that follows ({y0, x0, y1, x1, cost},
                // turned like 'X')
                if !r.has(2) {
                    break;
                }
                let cnt = r.u16() as usize;
                if !r.has(20 * cnt) {
                    break;
                }
                let rects = (0..cnt)
                    .map(|_| {
                        let (y0, x0, y1, x1) = (r.u32() as i32, r.u32() as i32, r.u32() as i32, r.u32() as i32);
                        [x0, y0, x1, y1, r.u32() as i32]
                    })
                    .collect();
                regions = Some(rects);
            }
            b'X' => {
                // the obstacles of the plan that follows
                if !r.has(6) {
                    break;
                }
                let width = r.u32() as i32;
                let cnt = r.u16() as usize;
                if !r.has(20 * cnt) {
                    break;
                }
                // stored {y0, x0, y1, x1, cost} (the footprint's words taken in reverse): kept
                // here as {x0, y0, x1, y1, cost}, the objects' and the waypoints' order
                let rects = (0..cnt)
                    .map(|_| {
                        let (y0, x0, y1, x1) = (r.u32() as i32, r.u32() as i32, r.u32() as i32, r.u32() as i32);
                        [x0, y0, x1, y1, r.u32() as i32]
                    })
                    .collect();
                obstacles = Some((width, rects));
            }
            b'H' => {
                // the request of the plan that follows
                if !r.has(10) {
                    break;
                }
                r.u32();
                let chosen = r.u32() as i32;
                let cnt = r.u16() as usize;
                if !r.has(16 * cnt) {
                    break;
                }
                let goals = (0..cnt).map(|_| [r.u32(), r.u32(), r.u32(), r.u32()]).collect();
                request = Some((chosen, goals));
            }
            b'R' => {
                // the random number autonomy's choice draws
                if !r.has(5) {
                    break;
                }
                let site = r.u8();
                log.randoms.push(RandomRec { tick: log.ticks, site, value: r.u32() });
            }
            b'W' => {
                // an evaluator's curves (tools/tree_log.py)
                if !r.has(6) {
                    break;
                }
                r.p += 4;
                let cnt = r.u16() as usize;
                if !r.has(cnt * (8 + 256 + 128)) {
                    break;
                }
                r.p += cnt * (8 + 256 + 128);
            }
            b'A' => {
                // an autonomy score
                if !r.has(12 + 64 + 132) {
                    break;
                }
                r.u32();
                r.u32();
                let mut s = ScoreRec { tick: log.ticks, score: r.f32(), ..Default::default() };
                for m in &mut s.motives {
                    *m = r.f32();
                }
                let a = &b[r.p..];
                for m in 0..16 {
                    for k2 in 0..3 {
                        s.adverts[m][k2] = i16::from_be_bytes([a[4 + 8 * m + 2 * k2], a[4 + 8 * m + 2 * k2 + 1]]);
                    }
                }
                r.p += 132;
                log.scores.push(s);
            }
            b'U' => {
                // a call of the motive update's gate (tools/tree_log.py)
                if !r.has(7) {
                    break;
                }
                r.p += 7;
            }
            b'N' => {
                // a person's motives changed (before the tick's 'K')
                if !r.has(66) {
                    break;
                }
                let mut m = MotiveRec { tick: log.ticks, id: r.i16(), ..Default::default() };
                for v in &mut m.motives {
                    *v = r.f32();
                }
                log.motives.push(m);
            }
            b'P' => {
                // an object's place changed (before the tick's 'K')
                if !r.has(14) {
                    break;
                }
                let id = r.i16();
                let (x, y, level) = (r.u32() as i32, r.u32() as i32, r.u32() as i32);
                log.places.push(PlaceRec { tick: log.ticks, id, x, y, level });
            }
            b'C' => {
                if !r.has(6) {
                    break;
                }
                r.p += 6;
            }
            b'M' => {
                // an object's memory (tools/tree_log.py)
                if !r.has(4 + 0x1100) {
                    break;
                }
                r.p += 4 + 0x1100;
            }
            b'O' => {
                // where an object keeps its definition (tools/definitions.py)
                if !r.has(4) {
                    break;
                }
                r.p += 4;
                let mut ended = false;
                while r.has(2) {
                    if r.i16() == 0x7FFF {
                        ended = true;
                        break;
                    }
                    if !r.has(202) {
                        break;
                    }
                    r.p += 202;
                }
                if !ended {
                    break;
                }
            }
            b'V' => {
                if !r.has(27) {
                    break;
                }
                pending_lookups.push(LookupRec {
                    object: r.u32(),
                    caller: r.u32(),
                    scope: r.i16(),
                    data: r.i16(),
                    value: r.i16(),
                    flags: r.u8(),
                    ptr: r.u32(),
                    fptr: r.u32(),
                    fvalue: r.u32(),
                });
            }
            b'E' => {
                if !r.has(28) {
                    break;
                }
                pending = Some(ExprRec {
                    lhs_scope: r.i16(),
                    lhs_data: r.i16(),
                    rhs_scope: r.i16(),
                    rhs_data: r.i16(),
                    op: r.u8(),
                    flags: r.u8(),
                    rhs: r.i16(),
                    lhs: r.i16(),
                    lhs_after: r.i16(),
                    lhs_float: r.u32(),
                    lhs_float_after: r.u32(),
                    result: r.u32(),
                });
            }
            b'T' => {
                if !r.has(12) {
                    break;
                }
                let nodes = r.u32();
                let tree = r.u16();
                let n = r.u16() as usize;
                let file = r.u32();
                if !r.has(n * 12) {
                    break;
                }
                let raw = b[r.p..r.p + n * 12].to_vec();
                r.p += n * 12;
                at.insert(nodes, log.trees.len());
                log.trees.push(TreeRec { nodes, tree, file, raw });
            }
            b'S' => {
                if !r.has(28) {
                    break;
                }
                let mut s = StepRec { sim: r.u32(), frame: r.u32(), file: r.u32(), nodes: r.u32(), ..Default::default() };
                s.tree = r.u16() & 0x7FFF;
                s.node = r.u16();
                s.after = r.u16();
                s.look = r.u8();
                s.result = r.u8();
                s.error = r.u16();
                let (np, nl) = (r.u8() as usize, r.u8() as usize);
                if !r.has(4 * (np + nl)) {
                    break;
                }
                s.params = (0..np).map(|_| r.i16()).collect();
                s.locals = (0..nl).map(|_| r.i16()).collect();
                s.params_after = (0..np).map(|_| r.i16()).collect();
                s.locals_after = (0..nl).map(|_| r.i16()).collect();
                if !r.has(20) {
                    break;
                }
                for d in &mut s.defaults {
                    *d = r.i16();
                }
                s.stack_object = r.i16();
                s.stack_object_after = r.i16();
                s.t = at.get(&s.nodes).copied();
                s.expr = pending.take();
                s.tick = log.ticks;
                s.in_pass = in_pass;
                s.lookups = std::mem::take(&mut pending_lookups);
                log.steps.push(s);
            }
            _ => {
                println!("bad record at {}", r.p - 1);
                return None;
            }
        }
    }
    Some(log)
}

/// The nodes as the original has them in memory: opcode, targets, and a call's operand (four
/// 16-bit fields; a primitive's operand fields are reordered on loading by their widths, which
/// are not all known, so those are not compared). Variants of an object can differ only in a
/// call's operand.
pub fn same_shape(t: &Tree, raw: &[u8]) -> bool {
    if raw.len() != t.nodes.len() * 12 {
        return false;
    }
    t.nodes.iter().zip(raw.as_chunks::<12>().0).all(|(n, r)| {
        u16::from_be_bytes([r[0], r[1]]) == n.opcode
            && r[2] == n.true_target
            && r[3] == n.false_target
            && ((n.opcode & 0x7FFF) <= 255 || (0..4).all(|k| u16::from_be_bytes([r[4 + 2 * k], r[5 + 2 * k]]) == n.operand16(k)))
    })
}
