//! The buy catalogue: the objects the buy screen lists, by category, with each one's name,
//! description and price (`docs/assets.md`, "Buy and Build").
//!
//! The data is the disc's own. An object file's definition (OBJD) has the price (field 16), the
//! category bits (field 38, the function sort), the catalogue strings' id (field 39) and what the
//! object does for a Sim's motives (fields 80 to 86); the strings the game shows are the quick
//! data's (`SimsObjects`' `ResFile` table: its row is named by the file, and its seventh word
//! points at the file's catalogue strings, each a chunk id, a name, a description and a kind).
//! They are not the object file's own catalogue strings (a few objects are named differently:
//! the PC's "Metal Dining Chair" is "Recalled Folding Chair" here).

use crate::library::{Definition, Library};
use crate::motives::motive;
use crate::quickdata::Image;

/// The buy screen's categories in the screen's order: the function-sort bit of the definitions
/// in it and the position of its name in `LiveModeMenuUIStrings`. The first is the log's
/// (buy-a.log: "Seating/Beds" is section 0); the order of the others is the PC game's category
/// order (unverified: the log never leaves the first).
pub const BUY_CATEGORIES: [(u16, usize); 9] = [(1, 138), (2, 152), (32, 57), (8, 63), (4, 26), (16, 110), (128, 82), (256, 141), (64, 88)];

/// The definition fields the catalogue reads (`Definition::field`).
const PRICE: usize = 16;
/// A multi-tile object's master id and its sub-index (65535 for the leader): only the leader is
/// sold.
const MASTER: usize = 8;
const SUB_INDEX: usize = 9;
/// A field that is 0 for the objects listed from the start (the Club Chair's is 275: it is the
/// one object of the 47 seats that the log's first section, 46 items, leaves out; taken to be
/// what keeps an object back until it is unlocked; unverified).
const LOCK: usize = 35;
const SORT: usize = 38;
const STRINGS: usize = 39;
/// The motive effects an object shows in its info window: the definition's fields 80 to 86,
/// hunger, comfort, hygiene, bladder, energy, fun and room (the stove's 5 is at 80, the
/// chair's 2 at 81, the bathtub's 8 and 10 at 81 and 82, the bed's 7 at 84).
const EFFECTS: [(usize, usize); 7] = [(80, motive::HUNGER), (81, motive::COMFORT), (82, motive::HYGIENE), (83, motive::BLADDER), (84, motive::ENERGY), (85, motive::FUN), (86, 13)];

/// One object of the catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The object's GUID (what the simulation makes it from).
    pub guid: u32,
    pub name: String,
    pub description: String,
    pub price: i32,
    /// The motives it feeds: the motive's place in a Sim's table and the amount (the info
    /// window's lozenges).
    pub effects: Vec<(usize, i32)>,
}

/// A purchase the buy screen hands its owner: what to make, where and which way it faces, and
/// what it costs. The screen never changes the world; the simulation applies it at its next tick
/// (`Alone::buy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Purchase {
    pub guid: u32,
    pub price: i32,
    /// The place in sixteenths of a tile and the level.
    pub at: (i32, i32),
    pub level: i32,
    pub facing: i16,
}

/// The items of each buy category, cheapest first.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub buy: Vec<Vec<Item>>,
}

impl Catalog {
    /// Reads the catalogue of a library from `quickdat.arc`'s bytes.
    pub fn read(lib: &Library, quick: &[u8]) -> Option<Catalog> {
        let entries = crate::arc::entries(quick)?;
        let e = entries.iter().find(|e| e.name == "SimsObjects")?;
        let img = Image::read_language(crate::arc::bytes(quick, e), 1)?;
        let mut buy: Vec<Vec<Item>> = vec![Vec::new(); BUY_CATEGORIES.len()];
        for f in lib.files() {
            let stem = f.path.rsplit('/').next().and_then(|n| n.strip_suffix(".iff").or_else(|| n.strip_suffix(".IFF"))).unwrap_or("");
            let Some(row) = img.row_named("ResFile", &stem.to_ascii_lowercase()).or_else(|| img.row_named("ResFile", stem)) else { continue };
            let Some(first) = img.ptr(row + 4 * 7) else { continue };
            // the file's own records: one a chunk, the next file's after them
            let records: Vec<(u32, String, String)> = (0..f.catalog.len())
                .filter_map(|k| {
                    let at = first + 20 * k;
                    Some((img.u32(at)?, img.text(at + 4)?, img.text(at + 8)?))
                })
                .collect();
            for d in &f.definitions {
                let sort = d.field(SORT);
                let for_sale = d.field(PRICE) > 0 && d.field(LOCK) == 0 && (d.field(MASTER) == 0 || d.field(SUB_INDEX) == 0xffff);
                let Some((_, name, description)) = records.iter().find(|r| r.0 == u32::from(d.field(STRINGS))).filter(|r| !r.1.is_empty()) else { continue };
                if !for_sale {
                    continue;
                }
                for (k, (bit, _)) in BUY_CATEGORIES.iter().enumerate() {
                    if sort & bit != 0 {
                        buy[k].push(item(d, name, description));
                    }
                }
            }
        }
        for items in &mut buy {
            // (a stable sort: objects of a price stay in the files' order)
            items.sort_by_key(|i| i.price);
        }
        Some(Catalog { buy })
    }

    /// The items of a category.
    pub fn items(&self, category: usize) -> &[Item] {
        self.buy.get(category).map_or(&[], Vec::as_slice)
    }
}

fn item(d: &Definition, name: &str, description: &str) -> Item {
    Item {
        guid: d.guid(),
        name: name.to_string(),
        description: description.to_string(),
        price: i32::from(d.field(PRICE)),
        effects: EFFECTS.iter().map(|&(f, m)| (m, i32::from(d.field(f) as i16))).filter(|e| e.1 != 0).collect(),
    }
}
