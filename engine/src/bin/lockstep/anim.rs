//! The animation studies (the LOCKSTEP_ANIM_* environment variables), one function each.

mod bones;
mod flags;
mod loads;
mod poses;
mod rotations;
mod runs;
mod streams;

pub use bones::bones;
pub use flags::flags;
pub use loads::loads;
pub use poses::poses;
pub use rotations::rotations;
pub use runs::runs;
pub use streams::channels;
