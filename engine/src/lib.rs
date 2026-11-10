//! House Fire: a native engine for the GameCube Sims games and The Sims 2: Castaway, whose game
//! logic is each game's own behaviour scripts read from the player's disc (`docs/plan.md`).

pub mod bhav;
pub mod disc;
pub mod expression;
pub mod iff;
pub mod library;
pub mod interp;
pub mod save;

/// A tree, by its place in the trees a host keeps (the library's, or a test's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TreeId(pub u32);

/// An object file, by its place in the library's files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);
