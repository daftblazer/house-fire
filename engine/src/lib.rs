//! House Fire: a native engine for the GameCube Sims games and The Sims 2: Castaway, whose game
//! logic is each game's own behaviour scripts read from the player's disc (`docs/plan.md`).

pub mod anim;
pub mod arc;
pub mod autonomy;
pub mod bhav;
pub mod cfmt;
pub mod disc;
pub mod draw;
pub mod drawlist;
pub mod expression;
pub mod figure;
pub mod floors;
pub mod iff;
pub mod library;
pub mod model;
pub mod outfit;
pub mod motives;
pub mod planner;
pub mod pose;
pub mod primitives;
pub mod replay;
pub mod rle_texture;
pub mod sim_texture;
pub mod quickdata;
pub mod scene;
pub mod shader;
pub mod skeleton;
pub mod skin;
pub mod routing;
pub mod interp;
pub mod lamps;
pub mod lighting;
pub mod light;
pub mod lotpick;
pub mod mathf;
pub mod save;
pub mod simulator;
pub mod texture;
pub mod tree_log;
pub mod world;

/// A tree, by its place in the trees a host keeps (the library's, or a test's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TreeId(pub u32);

/// An object file, by its place in the library's files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);
