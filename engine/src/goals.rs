//! The story's goals (`docs/primitives.md`, "CompleteGoal"): what each house asks of the player,
//! from the disc, and which goals the player has met, from the save.
//!
//! A goal is named by its house and its place in that house's list. Each goal has a mark, one
//! bit of the player's progress, and the progress is a set of such marks. The houses' lists
//! come from `SimsUI`'s `HouseData` table (`quickdata::house_goals`), and the progress from the
//! neighbourhood's NGBH chunk (`save::read_goal_marks`).

/// The goals of every house and the marks the player has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Goals {
    /// For each house (its number, the lot's global 10), each goal's mark.
    pub houses: Vec<Vec<u16>>,
    /// The marks met, as a bit set: mark m is bit m % 16 of word m / 16.
    pub met: Vec<u16>,
}

impl Goals {
    /// A goal's mark, or `None` if there is no such house or goal.
    pub fn mark(&self, house: i16, goal: i16) -> Option<u16> {
        let list = self.houses.get(usize::try_from(house).ok()?)?;
        list.get(usize::try_from(goal).ok()?).copied()
    }

    /// Whether the player has met this mark.
    pub fn is_met(&self, mark: u16) -> bool {
        self.met.get(mark as usize / 16).is_some_and(|w| w >> (mark % 16) & 1 != 0)
    }
}
