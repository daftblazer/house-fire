//! The light sets of the lot's draws, made from the engine's own state: the time of day's sun
//! and ambient, each room's ambient, and the lights (the lamps' and the windows') ranked into
//! the four an object at a place gets. Deterministic and GPU-free; the renderer turns a
//! `Lights` into its light set (`docs/assets.md`, "The frame from the engine's state").
//!
//! Built from what `light` ports: Light_TimeOfDay, Light_RoomAmbient, Light_Rebuild's sorting of
//! the lights, the nearest-light ranking and the window fill.

use crate::lamps::Lamp;
use crate::light::{self, LightKind};
use crate::quickdata::ObjResources;
use crate::world::World;
use std::collections::BTreeMap;

/// A light: its place (x, y, height), reach and colour (colour times factor, before the draw's
/// tint).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub at: [f32; 3],
    pub reach: f32,
    pub colour: [f32; 3],
}

/// The sun: its colour and the direction it shines in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    pub colour: [f32; 3],
    pub direction: [f32; 3],
}

/// What lights an object at a place: its room's ambient colour, the sun (outdoors) and the
/// four nearest lights.
#[derive(Clone, Debug, PartialEq)]
pub struct Lights {
    pub ambient: [f32; 3],
    pub sun: Option<Sun>,
    pub points: Vec<Source>,
}

/// The lot's lights.
#[derive(Clone)]
pub struct LotLights {
    /// The lights by index (the other lights from 0, the lamps' from 256); an unused slot is zero.
    sources: Vec<Source>,
    /// The sources' places and reaches, as the cell lookups take them.
    geometry: Vec<([f32; 3], f32)>,
    /// Each room's (other first, other end, own first, own end) in `sources`.
    ranges: Vec<[u32; 4]>,
    /// Each room's ambient colour.
    ambients: Vec<[f32; 3]>,
    sun: Sun,
    /// The original's room of each tile (the engine's less one; outside 0), `width` across.
    room_of: Vec<u8>,
    width: usize,
}

/// The lot's day and night ambient levels (`docs/assets.md`, "Light_RoomAmbient": home's and the
/// club's, kind 1 and 16 of the light manager).
pub const DAY_LEVEL: f32 = 0.3;
pub const NIGHT_LEVEL: f32 = 0.2;

impl LotLights {
    /// The lights of the lot at the clock (hour, minute, second). `lamps` are the lot's lamps
    /// (`lamps::on_lot`), `res` the objects' resources (a window's kind comes from its model's key).
    pub fn build(world: &World, res: &BTreeMap<u32, ObjResources>, lamps: &[Lamp], clock: (i32, i32, i32)) -> LotLights {
        let walls = &world.walls;
        let (width, height) = (walls.size().0 as usize, walls.size().1 as usize);
        let room_at = |x: i32, y: i32| i32::from(walls.room(x, y).wrapping_sub(1));
        let rooms = walls.rooms().max(1);
        let tod = light::time_of_day(clock.0, clock.1, clock.2);
        let sun_colour = tod.sun.map(|v| v * tod.sun_factor);
        let direction = light::sun_direction(clock.0, clock.1, clock.2).unwrap_or([0.0, 0.0, -1.0]);

        // the other lights: each window's two, one each side; the own lights: the lamps'
        let mut objs = Vec::new();
        let mut info = Vec::new();
        for o in world.objects().filter(|o| o.window && o.on_lot() && o.leader == o.id()) {
            let Some(places) = light::window_light_places((o.x16 >> 4, o.y16 >> 4), o.facing() as u8) else { continue };
            let key = world.objects().filter(|p| p.leader == o.id()).find_map(|p| res.get(&p.guid)).map(|r| r.key);
            let kind = key.map_or(0, light::window_kind);
            let at = |(x, y): (f32, f32)| room_at(x.floor() as i32, y.floor() as i32);
            for side in 0..2 {
                let (me, other) = (at(places[side]), at(places[1 - side]));
                objs.push((LightKind::Window, me, other));
                info.push((places[side], kind, me));
            }
        }
        let windows = objs.len();
        let by_leader: BTreeMap<i16, &Lamp> = lamps.iter().map(|l| (l.leader, l)).collect();
        let mut lamp_lights = Vec::new();
        for o in world.objects().filter(|o| o.on_lot()) {
            let Some(l) = by_leader.get(&o.id()) else { continue };
            let room = room_at(o.x16 >> 4, o.y16 >> 4);
            for light in &l.lights {
                objs.push((LightKind::Lamp, room, room));
                lamp_lights.push(*light);
            }
        }
        let (ranges, other, own) = light::sort_lights(rooms, &objs);
        let mut sources = vec![Source { at: [0.0; 3], reach: 0.0, colour: [0.0; 3] }; 512];
        for (k, &l) in own.iter().enumerate() {
            let lamp = &lamp_lights[l - windows];
            sources[256 + k] = Source { at: lamp.at, reach: lamp.reach, colour: lamp.colour };
        }
        let geometry = |sources: &[Source]| sources.iter().map(|s| (s.at, s.reach)).collect::<Vec<_>>();
        for (k, &i) in other.iter().enumerate() {
            let ((x, y), kind, me) = info[i];
            let Some((reach, base, Some(h))) = light::window_light_params(kind, me != 0) else { continue };
            let at = [x, y, h];
            // the fill: the sun when inside, the nearest lamps of the room on its other side
            let near = usize::try_from(objs[i].2).ok().and_then(|room| ranges.get(room)).and_then(|r| light::nearest_lights(at, (r[2], r[3]), &geometry(&sources)));
            let lamps_near: Vec<_> = near.map_or(Vec::new(), |n| n.iter().map_while(|s| s.map(usize::from)).map(|id| (sources[id].at, sources[id].reach, sources[id].colour)).collect());
            let mix = light::window_mix(me, sun_colour, at, &lamps_near);
            sources[k] = Source { at, reach, colour: mix.map(|v| v * base) };
        }
        // each room's ambient: the time of day's colour at the room's level (the outside's own
        // factor; an inside room's day or night level times its window brightness)
        let (mut tiles, mut openings) = (vec![0u32; rooms], vec![0u32; rooms]);
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let r = room_at(x, y);
                if let Some(t) = usize::try_from(r).ok().and_then(|r| tiles.get_mut(r)) {
                    *t += 1;
                }
            }
        }
        for o in world.objects().filter(|o| o.on_lot() && (o.door || o.window)) {
            if let Some(n) = usize::try_from(room_at(o.x16 >> 4, o.y16 >> 4)).ok().and_then(|r| openings.get_mut(r)) {
                *n += 1;
            }
        }
        let ambients = (0..rooms)
            .map(|r| {
                let factor = if r == 0 {
                    tod.ambient_factor
                } else {
                    light::room_ambient(DAY_LEVEL, NIGHT_LEVEL, clock.0, clock.1, clock.2, light::window_brightness(openings[r], tiles[r].max(1) as i32))
                };
                tod.ambient.map(|c| c * factor)
            })
            .collect();
        let room_of = (0..height as i32).flat_map(|y| (0..width as i32).map(move |x| (x, y))).map(|(x, y)| room_at(x, y) as u8).collect();
        let geometry = geometry(&sources);
        LotLights { sources, geometry, ranges, ambients, sun: Sun { colour: sun_colour, direction }, room_of, width }
    }

    /// The original's room of a tile (0 outside).
    pub fn room(&self, x: i32, y: i32) -> usize {
        usize::try_from(x).ok().zip(usize::try_from(y).ok()).and_then(|(x, y)| (x < self.width).then(|| self.room_of.get(y * self.width + x).copied()).flatten()).map_or(0, usize::from)
    }

    /// The colour base and scale of the floor groups of `room` made of a material with the
    /// diffuse and ambient triples `material` (`Shader::material`): from the room's ambient and,
    /// outdoors, the sun's light times its downward part (the original's Light_RoomTriples and
    /// Light_GroupScale: only room 0 has the sun).
    pub fn floor_group(&self, room: usize, material: ([f32; 3], [f32; 3])) -> ([f32; 3], [f32; 3]) {
        let ambient = self.ambients.get(room).copied().unwrap_or([light::NO_LIGHT; 3]);
        let light = if room == 0 {
            let down = (-self.sun.direction[2]).max(light::NO_LIGHT);
            self.sun.colour.map(|c| (f64::from(down) * f64::from(c)) as f32)
        } else {
            [light::NO_LIGHT; 3]
        };
        light::group_colour(material.0, material.1, ambient, light)
    }

    /// The colours of floor corners `points` (grid points: x, y) of `room`, for floor groups with
    /// `group` (`floor_group`): each the light grid's sum at the corner over the lights of the
    /// cell's room (`light::cell_lights`), as `light::vertex_colour` makes the byte colour.
    pub fn floor_colours(&self, room: usize, group: &([f32; 3], [f32; 3]), points: &[(i32, i32)]) -> Vec<[u8; 4]> {
        self.floor_lit(group, points, &self.floor_cells(room, points))
    }

    /// The light grid's cells (`light::cell_lights`) of floor corners `points` for `room`: which
    /// lights reach each. They depend on the lights' places and reaches (`same_geometry`), not on
    /// their colours, so a floor keeps them while the time of day changes the colours.
    pub fn floor_cells(&self, room: usize, points: &[(i32, i32)]) -> Vec<Option<[u16; 4]>> {
        let ranges = self.ranges.get(room).copied();
        points.iter().map(|&(x, y)| ranges.and_then(|r| light::cell_lights([x as f32, y as f32, light::GRID_Z], (r[2], r[3]), (r[0], r[1]), &self.geometry))).collect()
    }

    /// The colours of floor corners `points` with their `cells` (`floor_cells`).
    pub fn floor_lit(&self, group: &([f32; 3], [f32; 3]), points: &[(i32, i32)], cells: &[Option<[u16; 4]>]) -> Vec<[u8; 4]> {
        points
            .iter()
            .zip(cells)
            .map(|(&(x, y), ids)| {
                let mut lights = [None; 4];
                let mut n = 0;
                for &i in ids.iter().flatten().filter(|&&i| i != 0xFFFF) {
                    let s = &self.sources[usize::from(i)];
                    lights[n] = Some(light::Light { x: s.at[0], y: s.at[1], height: s.at[2], reach: s.reach, colour: s.colour });
                    n += 1;
                }
                let lights: [light::Light; 4] = lights.map(|l| l.unwrap_or(light::Light { x: 0.0, y: 0.0, height: 0.0, reach: 0.0, colour: [0.0; 3] }));
                light::vertex_colour(light::point_light(x as f32, y as f32, &lights[..n]), group.0, group.1)
            })
            .collect()
    }

    /// Whether `other` lights the same cells as these lights (the same places, reaches, ranges and
    /// rooms; the colours and the sun may differ).
    pub fn same_geometry(&self, other: &LotLights) -> bool {
        self.width == other.width && self.ranges == other.ranges && self.room_of == other.room_of && self.geometry == other.geometry
    }

    /// The four lights (source indices, 0xFFFF for an empty slot) of the light grid's cell at
    /// `point` for `room`.
    fn cell(&self, room: usize, point: [f32; 2]) -> Option<[u16; 4]> {
        let r = self.ranges.get(room)?;
        light::cell_lights([point[0], point[1], light::GRID_Z], (r[2], r[3]), (r[0], r[1]), &self.geometry)
    }

    /// What lights a fence at (x, y) whose edge starts at `start`: the four lights of the light
    /// grid's point there, as an object's, in the room of the tile.
    pub fn fence(&self, x: f32, y: f32, start: [f32; 2]) -> Lights {
        let room = self.room(x.floor() as i32, y.floor() as i32);
        let points = self.cell(room, start).map_or(Vec::new(), |ids| ids.iter().filter(|&&i| i != 0xFFFF).map(|&i| self.sources[usize::from(i)]).collect());
        Lights { ambient: self.ambients.get(room).copied().unwrap_or([0.0; 3]), sun: (room == 0).then_some(self.sun), points }
    }

    /// The two of a cell's four lights a wall piece takes at an end: the strongest, and the
    /// first of the next two that is not of its kind (a lamp, an own light from 256, or a
    /// window's other light), else the second. Fitted to the log's wall sets (home, frame 2400:
    /// all 84 wall, diagonal and fence sets); the original's reason for slots 0 to 2 only (so a lamp
    /// ranked last is left out beside three windows) is not known.
    fn end_pair(slots: [u16; 4]) -> [u16; 2] {
        let own = |i: u16| i >= 256 && i != 0xFFFF;
        let second = slots[1..3].iter().copied().find(|&i| own(i) != own(slots[0])).unwrap_or(slots[1]);
        [slots[0], second]
    }

    /// The room an object at (x, y) is in (`room_at`: the tile before an edge).
    pub fn room_of_place(&self, x: f32, y: f32) -> usize {
        self.room_at(x, y)
    }

    /// What lights a draw of the lot's list: a wall piece or fence along its edge, anything else
    /// at its place, in the room of its mesh's middle (a window's frame and glass are on the two
    /// sides of the wall, so they take the two rooms' sets: the log's windows, all 30 meshes).
    pub fn of(&self, d: &crate::drawlist::Draw) -> Lights {
        match d.edge {
            // a fence stands on its edge (its place is the edge's middle), a wall piece's place is
            // its tile's centre, half a tile off the edge
            Some(ends) if (0..2).all(|k| (d.origin[k] - 0.5 * (ends[0][k] + ends[1][k])).abs() < 1e-3) => self.fence(d.origin[0], d.origin[1], ends[0]),
            Some(ends) => self.along(d.origin[0], d.origin[1], ends),
            None => self.at_in(d.centre, d.origin),
        }
    }

    /// What lights a wall piece (or diagonal) on the edge from `ends[0]` to `ends[1]`, in the room
    /// of the tile (x, y): two lights from the light grid's point at each end (`end_pair`; the
    /// log's sets are such a pair from each end, the same light twice where the ends share it).
    pub fn along(&self, x: f32, y: f32, ends: [[f32; 2]; 2]) -> Lights {
        let room = self.room(x.floor() as i32, y.floor() as i32);
        let mut points = Vec::new();
        for e in ends {
            points.extend(self.cell(room, e).into_iter().flat_map(Self::end_pair).filter(|&i| i != 0xFFFF).map(|i| self.sources[usize::from(i)]));
        }
        Lights { ambient: self.ambients.get(room).copied().unwrap_or([0.0; 3]), sun: (room == 0).then_some(self.sun), points }
    }

    /// The room an object at (x, y) is in. On a tile's edge (a door's or window's place: x or y a
    /// whole number) it is in the tile before the edge, or in the one after it when that is
    /// outdoors (a window in a wall at x = 13 looks into the room east of it).
    fn room_at(&self, x: f32, y: f32) -> usize {
        let (cx, cy) = (x.ceil() as i32 - 1, y.ceil() as i32 - 1);
        let before = self.room(cx, cy);
        if before != 0 {
            return before;
        }
        // the tiles across the edge(s): the one after in x when x is whole, in y when y is whole
        let (ax, ay) = (x.fract() == 0.0, y.fract() == 0.0);
        let across = [(ax, cx + 1, cy), (ay, cx, cy + 1), (ax && ay, cx + 1, cy + 1)];
        across.into_iter().filter(|a| a.0).map(|a| self.room(a.1, a.2)).find(|&r| r != 0).unwrap_or(0)
    }

    /// What lights an object at (x, y) on the lot: its room's ambient, the sun if the room is
    /// outside, and its four lights (`light::cell_lights` at the light grid's point nearest to it,
    /// whole tiles: the room's own lights first, then its windows', the strongest first; 72 of
    /// the home lot's 98 object places give the log's four lights, `docs/assets.md`).
    pub fn at(&self, x: f32, y: f32) -> Lights {
        self.at_in([x, y], [x, y])
    }

    /// `at` for a mesh whose middle `centre` is in the room (its ambient, the sun) and whose object
    /// is at `origin` (the light grid's point).
    pub fn at_in(&self, centre: [f32; 2], origin: [f32; 2]) -> Lights {
        let (room, (x, y)) = (self.room_at(centre[0], centre[1]), (origin[0], origin[1]));
        let points = self.cell(room, [x.round(), y.round()]).map_or(Vec::new(), |ids| ids.iter().filter(|&&i| i != 0xFFFF).map(|&i| self.sources[usize::from(i)]).collect());
        Lights { ambient: self.ambients.get(room).copied().unwrap_or([0.0; 3]), sun: (room == 0).then_some(self.sun), points }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 x 4 lot whose tile (1, 3) is room 1 and the rest outside, with no lights.
    fn lot() -> LotLights {
        let mut room_of = vec![0u8; 16];
        room_of[3 * 4 + 1] = 1;
        LotLights {
            sources: vec![Source { at: [0.0; 3], reach: 0.0, colour: [0.0; 3] }; 512],
            geometry: vec![([0.0; 3], 0.0); 512],
            ranges: vec![[0, 0, 256, 256], [0, 0, 256, 256]],
            ambients: vec![[0.1; 3], [0.2; 3]],
            sun: Sun { colour: [1.0; 3], direction: [0.0, 0.0, -1.0] },
            room_of,
            width: 4,
        }
    }

    #[test]
    fn an_object_on_a_tiles_edge_is_in_the_tile_before_it() {
        let l = lot();
        // a door at (2.0, 3.5) sits on the edge of tiles 1 and 2: the one before it, room 1
        assert_eq!(l.at(2.0, 3.5).ambient, [0.2; 3]);
        assert!(l.at(2.0, 3.5).sun.is_none());
        // inside a tile: that tile's room; outside is room 0 and has the sun
        assert_eq!(l.at(2.5, 3.5).ambient, [0.1; 3]);
        assert!(l.at(2.5, 3.5).sun.is_some());
    }

    #[test]
    fn an_object_on_an_edge_takes_the_room_across_it_when_the_tile_before_is_outside() {
        let l = lot();
        // a window at (1.0, 3.5) is on the edge of tiles 0 (outside) and 1 (room 1): room 1
        assert_eq!(l.at(1.0, 3.5).ambient, [0.2; 3]);
        // and at (1.5, 4.0) the tile before it in y is room 1 already
        assert_eq!(l.at(1.5, 4.0).ambient, [0.2; 3]);
        // an edge with outdoors on both sides stays outdoors
        assert_eq!(l.at(3.0, 0.5).ambient, [0.1; 3]);
    }

    #[test]
    fn a_mesh_takes_the_room_of_its_middle_not_of_its_objects_place() {
        let l = lot();
        // a window at (1.0, 3.5): the frame's middle is a little west of the edge (outdoors), the
        // glass's a little east (room 1)
        assert_eq!(l.at_in([0.97, 3.5], [1.0, 3.5]).ambient, [0.1; 3]);
        assert!(l.at_in([0.97, 3.5], [1.0, 3.5]).sun.is_some());
        assert_eq!(l.at_in([1.03, 3.5], [1.0, 3.5]).ambient, [0.2; 3]);
        assert!(l.at_in([1.03, 3.5], [1.0, 3.5]).sun.is_none());
    }

    #[test]
    fn a_floor_without_lights_is_the_groups_base_and_only_outdoors_has_the_suns_light() {
        let l = lot();
        // a material with a diffuse of 0.5 and no ambient: outdoors the sun's light (1, straight
        // down) times 0.5 times the group's constant (128), indoors none
        let material = ([0.5; 3], [0.0; 3]);
        let outdoors = l.floor_group(0, material);
        assert_eq!(l.floor_colours(0, &outdoors, &[(1, 1)]), vec![[64, 64, 64, 0x80]]);
        let indoors = l.floor_group(1, material);
        assert_eq!(l.floor_colours(1, &indoors, &[(1, 3), (2, 3)]), vec![[0, 0, 0, 0x80]; 2]);
    }

    #[test]
    fn a_tile_off_the_lot_is_outside() {
        let l = lot();
        assert_eq!(l.room(-1, 0), 0);
        assert_eq!(l.room(9, 9), 0);
        assert_eq!(l.room(1, 3), 1);
    }

    #[test]
    fn a_wall_takes_the_strongest_light_and_the_first_other_kind_of_the_next_two() {
        // windows (below 256) and lamps (from 256): [window, window, lamp, lamp] takes the lamp, a lamp
        // ranked last beside three windows is left out
        assert_eq!(LotLights::end_pair([1, 2, 256, 257]), [1, 256]);
        assert_eq!(LotLights::end_pair([1, 2, 3, 256]), [1, 2]);
        assert_eq!(LotLights::end_pair([256, 1, 2, 3]), [256, 1]);
        assert_eq!(LotLights::end_pair([1, 256, 2, 257]), [1, 256]);
    }
}
