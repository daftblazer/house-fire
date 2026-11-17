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
pub struct LotLights {
    /// The lights by index (the other lights from 0, the lamps' from 256); an unused slot is zero.
    sources: Vec<Source>,
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
        LotLights { sources, ranges, ambients, sun: Sun { colour: sun_colour, direction }, room_of, width }
    }

    /// The original's room of a tile (0 outside).
    pub fn room(&self, x: i32, y: i32) -> usize {
        usize::try_from(x).ok().zip(usize::try_from(y).ok()).and_then(|(x, y)| (x < self.width).then(|| self.room_of.get(y * self.width + x).copied()).flatten()).map_or(0, usize::from)
    }

    /// What lights an object at (x, y) on the lot: its room's ambient, the sun if the room is
    /// outside, and its four lights (`light::cell_lights`: the room's own lights first, then
    /// its windows', the strongest first).
    pub fn at(&self, x: f32, y: f32) -> Lights {
        let room = self.room(x.floor() as i32, y.floor() as i32);
        let geometry: Vec<([f32; 3], f32)> = self.sources.iter().map(|s| (s.at, s.reach)).collect();
        let points = self
            .ranges
            .get(room)
            .and_then(|r| light::cell_lights([x, y, light::GRID_Z], (r[2], r[3]), (r[0], r[1]), &geometry))
            .map_or(Vec::new(), |ids| ids.iter().filter(|&&i| i != 0xFFFF).map(|&i| self.sources[usize::from(i)]).collect());
        Lights { ambient: self.ambients.get(room).copied().unwrap_or([0.0; 3]), sun: (room == 0).then_some(self.sun), points }
    }
}
