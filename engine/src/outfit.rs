//! What a Sim wears: its description (from the save) and the part tables of its age and gender
//! give the model of each body part, a `models.arc` entry by name hash (`shader::name_hash`).
//!
//! Simulation-side data (a server dresses Sims too): the tables are read once from the quick
//! data's `CreateASim` image into plain vectors, and dressing a Sim is nine array reads, with no
//! reference to a player or a renderer.
//!
//! Which tables: `CreateASim`'s `Sim::Table` has a row for each of "AdultMale", "AdultFemale",
//! "ChildMale" and "ChildFemale" (and others, not read). A row holds nine arrays, one for each
//! part slot; an array is indexed by a byte of the description, and the first word of its entry
//! (4, 12 or 16 bytes, by the slot) is the part's name hash, 0 for no part.

use crate::quickdata::Image;
use crate::save::CreatedSim;

/// The body parts a Sim wears, one model each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Gloves or jewellery (the model names start `gl_` or `jw_`).
    Accessory,
    /// What is worn on the head or the hair (`hh_`).
    Head,
    /// The upper body (`ub_`).
    Upper,
    /// The lower body (`lb_`).
    Lower,
    /// The shoes (`sh_`).
    Shoes,
    Eyes,
    Nose,
    Mouth,
    /// A ninth array of the table; every entry of it seen is empty.
    Extra,
}

impl Slot {
    pub const ALL: [Slot; 9] = [Slot::Accessory, Slot::Head, Slot::Upper, Slot::Lower, Slot::Shoes, Slot::Eyes, Slot::Nose, Slot::Mouth, Slot::Extra];

    /// Where the slot's array is in a `Sim::Table` row (its pointer's offset), the bytes in an
    /// entry, and which byte of the description (counted as the original's description does,
    /// word 0 the gender, word 1 the age, the saved looks from byte 8) indexes it. From the
    /// original's dressing code (Sim_SetModels at 800591CC, 80059DC4 to 80059E98).
    fn table(self) -> (usize, usize, usize) {
        match self {
            Slot::Accessory => (28, 4, 9),
            Slot::Head => (24, 16, 11),
            Slot::Upper => (0, 16, 12),
            Slot::Lower => (4, 16, 13),
            Slot::Shoes => (8, 12, 14),
            Slot::Eyes => (12, 16, 23),
            Slot::Nose => (16, 12, 24),
            Slot::Mouth => (20, 12, 25),
            Slot::Extra => (32, 16, 15),
        }
    }
}

/// A Sim's description: its gender and age, and the bytes of looks that follow them in the save
/// (20 bytes, `CreatedSim::looks`); only the part indices are named so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Description {
    pub male: bool,
    pub adult: bool,
    /// Bytes 8 to 25 of the description: the part indices and 8 or so bytes not understood yet
    /// (colours, most likely).
    pub looks: [u8; 18],
}

/// The description's first byte of the saved looks, as the original counts it.
const FIRST_LOOK: usize = 8;

impl Description {
    /// The description as the observer logged it at a Sim's dressing: the first 16 words of the
    /// guest's description (big-endian words of the same bytes the save keeps: gender, age, then
    /// the looks from byte 8; the gender and age are words of their own).
    pub fn from_words(words: &[u32; 16]) -> Description {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
        let mut looks = [0; 18];
        looks.copy_from_slice(&bytes[FIRST_LOOK..FIRST_LOOK + 18]);
        Description { male: words[0] != 0, adult: words[1] != 0, looks }
    }

    pub fn from_save(sim: &CreatedSim) -> Description {
        let mut looks = [0; 18];
        looks.copy_from_slice(&sim.looks[2..]);
        Description { male: sim.looks[0] != 0, adult: sim.looks[1] != 0, looks }
    }

    /// The part index the slot's array is read at (a byte, as the original reads it signed).
    pub fn index(&self, slot: Slot) -> i8 {
        self.looks[slot.table().2 - FIRST_LOOK] as i8
    }
}

/// The arrays of one table row: each slot's part hashes by index.
#[derive(Clone, Debug, Default)]
struct PartSet {
    /// Each entry's first four words (its name hash, then what the entry keeps after it: the
    /// textures of a part's layers); words past the entry's size are 0.
    parts: [Vec<[u32; 4]>; 9],
}

/// What a Sim wears: each slot's part (a name hash, 0 for none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outfit {
    parts: [u32; 9],
}

impl Outfit {
    pub fn part(&self, slot: Slot) -> u32 {
        self.parts[slot as usize]
    }
}

/// The part tables of the four ages and genders.
#[derive(Clone, Debug, Default)]
pub struct PartTables {
    /// By `adult as usize * 2 + male as usize`.
    sets: [PartSet; 4],
}

/// The `Sim::Table` rows' names, by `adult as usize * 2 + male as usize`.
const SET_NAMES: [&str; 4] = ["ChildFemale", "ChildMale", "AdultFemale", "AdultMale"];

impl PartTables {
    /// The tables from the quick data's `CreateASim` image, or `None` if it has none of them.
    pub fn read(img: &Image) -> Option<PartTables> {
        // (a row is nine pointers)
        img.tables.get("Sim::Table").filter(|t| t.row_size >= 36)?;
        // An array runs to the next one's start (all rows' arrays lie in one run).
        let mut starts: Vec<usize> = img.rows("Sim::Table").flat_map(|r| Slot::ALL.map(|s| img.ptr(r + s.table().0))).flatten().collect();
        starts.sort_unstable();
        let mut tables = PartTables::default();
        for (k, name) in SET_NAMES.iter().enumerate() {
            let row = img.row_named("Sim::Table", name)?;
            for slot in Slot::ALL {
                let (at, stride, _) = slot.table();
                let start = img.ptr(row + at)?;
                let end = starts.iter().copied().find(|&s| s > start).unwrap_or(img.data.len());
                tables.sets[k].parts[slot as usize] = (start..end.saturating_sub(stride - 1))
                    .step_by(stride)
                    .map(|e| std::array::from_fn(|w| if w * 4 < stride { img.u32(e + w * 4).unwrap_or(0) } else { 0 }))
                    .collect();
            }
        }
        Some(tables)
    }

    /// The parts a Sim of this description wears; a part the table has no entry for is none.
    pub fn dress(&self, d: &Description) -> Outfit {
        let set = &self.sets[d.adult as usize * 2 + d.male as usize];
        let mut o = Outfit::default();
        for slot in Slot::ALL {
            let i = d.index(slot);
            o.parts[slot as usize] = usize::try_from(i).ok().and_then(|i| set.parts[slot as usize].get(i)).map_or(0, |e| e[0]);
        }
        o
    }

    /// The table entry the description picks for a slot: the part's name hash, then the words
    /// that follow it (the run-length textures of the part's layers, `sim_texture`); `None` for
    /// an index the table has no entry for.
    pub fn entry(&self, d: &Description, slot: Slot) -> Option<[u32; 4]> {
        let set = &self.sets[d.adult as usize * 2 + d.male as usize];
        usize::try_from(d.index(slot)).ok().and_then(|i| set.parts[slot as usize].get(i)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_from_the_save() {
        // a made Sim's looks in the club save: male, adult, then the part indices
        let looks = [1, 1, 0, 0x24, 0x0e, 0x52, 0x1e, 0x1d, 0x0b, 0x12, 1, 5, 0x1f, 1, 1, 2, 0x0f, 1, 0, 4];
        let d = Description::from_save(&CreatedSim { looks, ..Default::default() });
        assert!(d.male && d.adult);
        assert_eq!((d.index(Slot::Accessory), d.index(Slot::Head), d.index(Slot::Upper)), (0x24, 0x52, 0x1e));
        assert_eq!((d.index(Slot::Eyes), d.index(Slot::Nose), d.index(Slot::Mouth)), (1, 0, 4));
    }

    #[test]
    fn a_missing_part_is_none() {
        let d = Description { looks: [0x80; 18], ..Default::default() };
        assert_eq!(PartTables::default().dress(&d), Outfit::default());
    }
}
