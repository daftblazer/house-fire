
use crate::disc::Disc;
use crate::goals::Goals;
use crate::save::{LotChunks, Neighbour, memory_card_file, read_goal_marks, read_families, read_lot_globals, read_neighbourhood_data, read_lots, read_neighbours};
use super::study::has;

/// The save a run starts from: its lots, their globals and the neighbours, and which lot the
/// run is on (a memory card's first, unless `lot_index` says; else the disc's own default save,
/// the lot at `lot_index` or the last).
pub struct Start {
    pub lots: Vec<LotChunks>,
    pub lot_globals: Vec<Vec<i16>>,
    pub neighbours: Vec<Neighbour>,
    /// The lot's place in `lots`.
    pub index: usize,
    /// Whether the save is a memory card's (its motives run: `World::motives_run`).
    pub card_lot: bool,
    /// The disc's copy of the lot (its house: walls and floors a card's save may not keep).
    pub house: Option<LotChunks>,
    /// The houses' goals (the disc's `SimsUI`) and those the save has met.
    pub goals: Goals,
    /// The neighbourhood's data (scope 34), as saved.
    pub neighbourhood: Vec<i16>,
    /// The families of the save (the pause screen shows the family's funds).
    pub families: Vec<crate::save::Family>,
}

impl Start {
    /// Reads the save: `card` a memory card image (the Bustin' Out save on it), else the disc's
    /// `Runtime/default.ngh`. A negative `lot_index` is the first lot of a card's, else the last.
    pub fn read(disc: &Disc, card: Option<&str>, mut lot_index: i64) -> Result<Start, String> {
        let ngh = match card {
            Some(p) => {
                let mut bytes = std::fs::read(p).unwrap_or_default();
                bytes.truncate(1 << 22);
                let ngh = memory_card_file(&bytes, "G4ME69").unwrap_or_default();
                if ngh.is_empty() {
                    return Err(format!("no Bustin' Out save on {p}"));
                }
                if lot_index < 0 {
                    lot_index = 0;
                }
                ngh
            }
            None => disc.read("Runtime/default.ngh").unwrap_or_default(),
        };
        let lots = read_lots(&ngh).map_err(|e| format!("the lots: {e:?}"))?;
        let neighbours = read_neighbours(&ngh).map_err(|e| format!("the neighbours: {e:?}"))?;
        if lots.is_empty() {
            return Err("no lots".into());
        }
        let lot_globals = read_lot_globals(&ngh).map_err(|e| format!("the lots' globals: {e:?}"))?;
        let index = if lot_index < 0 || lot_index as usize >= lots.len() { lots.len() - 1 } else { lot_index as usize };
        let card_lot = card.is_some();
        let disc_lots = if card_lot && !has("REPLAY_NO_DISC_HOUSE") { read_lots(&disc.read("Runtime/default.ngh").unwrap_or_default()).unwrap_or_default() } else { Vec::new() };
        let house = usize::try_from(lot_index).ok().and_then(|i| disc_lots.get(i).cloned());
        let met = read_goal_marks(&ngh).map_err(|e| format!("the goals: {e:?}"))?;
        let neighbourhood = read_neighbourhood_data(&ngh).map_err(|e| format!("the neighbourhood's data: {e:?}"))?;
        let families = read_families(&ngh).unwrap_or_default();
        let quick = disc.read("quickdat.arc").unwrap_or_default();
        let houses = crate::arc::entries(&quick)
            .unwrap_or_default()
            .into_iter()
            .find(|e| e.name == "SimsUI")
            .and_then(|e| crate::quickdata::Image::read(crate::arc::bytes(&quick, &e)))
            .map(|img| crate::quickdata::house_goals(&img))
            .unwrap_or_default();
        Ok(Start { lots, lot_globals, neighbours, index, card_lot, house, goals: Goals { houses, met }, neighbourhood, families })
    }

    /// The neighbourhood (its data, scope 34, and the neighbours) from another save, a memory card's: a disc lot (the
    /// club, `--lot 15`) visited from that save's neighbourhood.
    pub fn hood_from(&mut self, card: &str) -> Result<(), String> {
        let mut bytes = std::fs::read(card).map_err(|e| format!("{card}: {e}"))?;
        bytes.truncate(1 << 22);
        let ngh = memory_card_file(&bytes, "G4ME69").unwrap_or_default();
        if ngh.is_empty() {
            return Err(format!("no Bustin' Out save on {card}"));
        }
        self.neighbourhood = read_neighbourhood_data(&ngh).map_err(|e| format!("the neighbourhood's data: {e:?}"))?;
        self.neighbours = read_neighbours(&ngh).map_err(|e| format!("the neighbours: {e:?}"))?;
        Ok(())
    }

    /// The funds the save keeps for the family of the lot's house: the second word of its
    /// record (the pause screen's second funds line; 0 for a house with no family).
    pub fn family_funds(&self) -> i32 {
        let house = i32::from(self.globals().get(10).copied().unwrap_or(0));
        self.families.iter().find(|f| f.house == house).map_or(0, |f| f.words[1])
    }

    pub fn lot(&self) -> &LotChunks {
        &self.lots[self.index]
    }

    /// The lot's globals as saved (its SIMI chunk): the clock among them.
    pub fn globals(&self) -> &[i16] {
        // (a lot index past the globals' list takes the last, as the lot index does for the lots)
        let g = &self.lot_globals;
        if self.index >= g.len() { g.last().map_or(&[], |g| g) } else { &g[self.index] }
    }
}

/// What a run starts with besides the log, the lot and the library: whether the lot is the
/// memory card's, and the disc's animations (for a log with the frames).
pub struct RunWith {
    pub card_lot: bool,
    pub goals: Goals,
    /// The neighbourhood's data (scope 34), as saved.
    pub neighbourhood: Vec<i16>,
    pub cat: Option<crate::anim::Catalog>,
}
