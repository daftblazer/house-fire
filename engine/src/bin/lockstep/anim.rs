//! The animation studies (the LOCKSTEP_ANIM_* environment variables), one function each.

mod loads;
mod runs;
mod streams;

pub use loads::loads;
pub use runs::runs;
pub use streams::channels;
