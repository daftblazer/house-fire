//! The animation studies (the LOCKSTEP_ANIM_* environment variables), one function each.

mod bones;
mod draws;
mod flags;
mod lamps;
mod outfits;
mod loads;
mod poses;
mod rotations;
mod runs;
mod sims;
mod skin;
mod streams;

pub use bones::bones;
pub use draws::draws;
pub use flags::flags;
pub use lamps::{engine_lamps, lamps};
pub use outfits::outfits;
pub use loads::loads;
pub use poses::poses;
pub use rotations::rotations;
pub use runs::runs;
pub use sims::sims;
pub use skin::skin_study;
pub use streams::channels;
