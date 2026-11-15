//! The animation studies (the LOCKSTEP_ANIM_* environment variables), one function each.

mod flags;
mod loads;
mod rotations;
mod runs;
mod streams;

pub use flags::flags;
pub use loads::loads;
pub use rotations::rotations;
pub use runs::runs;
pub use streams::channels;
