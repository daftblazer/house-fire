//! The lot's floors as the original draws them: one strip a run of tiles (`scene::floor_runs`, and
//! `scene::terrain_runs` for the bare ground), each corner with its colour (the lighting, from
//! the light grid, `lighting::LotLights::floor_colours`), grouped by the room the run is in
//! (`docs/assets.md`, "Floors"). Deterministic and GPU-free.

use crate::lighting::LotLights;
use crate::quickdata::FloorPattern;
use crate::routing::Walls;
use crate::scene::{floor_runs, terrain_runs, FloorRun};

/// A floor corner: its place in tiles (z 0), its texture coordinates and its colour (RGBA, the
/// alpha 0x80).
pub type Corner = ([f32; 3], [f32; 2], [u8; 4]);

/// A run of floor with its corners.
#[derive(Clone, Debug)]
pub struct Strip {
    pub run: FloorRun,
    /// The original's room of the run's first tile (0 outdoors): its batch.
    pub room: usize,
    /// The shader (`shaders.arc` id) the run is drawn with: its pattern's, or the terrain's.
    pub shader: u32,
    pub corners: Vec<Corner>,
    /// The light grid's cells of the corners (`LotLights::floor_cells`), for `relight`.
    cells: Vec<Option<[u16; 4]>>,
}

/// The lot's strips, rooms in order: the patterns' runs, then the terrain's, each in row order
/// (the batches' order within a room is not the original's, which groups a room's runs by
/// material: it does not show). `material` gives a shader's diffuse and ambient triples
/// (`Shader::material`); `size` the tiles across the lot to cover (the light grid's points: the
/// original draws no floor past them).
pub fn strips(walls: &Walls, size: i32, patterns: &[FloorPattern], terrain: u32, lights: &LotLights, material: impl Fn(u32) -> Option<([f32; 3], [f32; 3])>) -> Vec<Strip> {
    let runs = floor_runs(walls, size, patterns).into_iter().map(|r| (r, patterns[usize::from(r.value)].shader)).chain(terrain_runs(walls).into_iter().map(|r| (r, terrain)));
    let mut sums = Sums::default();
    let mut out: Vec<Strip> = runs
        .map(|(run, shader)| {
            let room = lights.room(run.x, run.y);
            let vertices = run.vertices();
            let group = lights.floor_group(room, material(shader).unwrap_or(([1.0; 3], [0.0; 3])));
            let points: Vec<(i32, i32)> = vertices.iter().map(|v| (v.0[0] as i32, v.0[1] as i32)).collect();
            let cells = lights.floor_cells(room, &points);
            let colours = sums.colours(lights, room, &group, &points, &cells);
            let corners = vertices.into_iter().zip(colours).map(|((p, uv), c)| (p, uv, c)).collect();
            Strip { run, room, shader, corners, cells }
        })
        .collect();
    out.sort_by_key(|s| s.room);
    out
}

/// The strips of `before` with their colours made again from `lights`, which must light the same
/// cells as the lights they were made with (`LotLights::same_geometry`) over the same walls.
pub fn relight(before: &[Strip], lights: &LotLights, material: impl Fn(u32) -> Option<([f32; 3], [f32; 3])>) -> Vec<Strip> {
    let mut sums = Sums::default();
    before
        .iter()
        .map(|s| {
            let group = lights.floor_group(s.room, material(s.shader).unwrap_or(([1.0; 3], [0.0; 3])));
            let points: Vec<(i32, i32)> = s.corners.iter().map(|c| (c.0[0] as i32, c.0[1] as i32)).collect();
            let colours = sums.colours(lights, s.room, &group, &points, &s.cells);
            let corners = s.corners.iter().zip(colours).map(|(&(p, uv, _), c)| (p, uv, c)).collect();
            Strip { corners, ..s.clone() }
        })
        .collect()
}

/// The light on each floor corner already summed, by room and grid point: the strips share their
/// corners, and the light at one does not depend on the group's material.
#[derive(Default)]
struct Sums {
    rooms: Vec<Vec<Option<[f32; 3]>>>,
}

impl Sums {
    fn colours(&mut self, lights: &LotLights, room: usize, group: &([f32; 3], [f32; 3]), points: &[(i32, i32)], cells: &[Option<[u16; 4]>]) -> Vec<[u8; 4]> {
        const N: usize = crate::light::GRID_SIZE;
        if self.rooms.len() <= room {
            self.rooms.resize(room + 1, Vec::new());
        }
        let table = &mut self.rooms[room];
        if table.is_empty() {
            table.resize(N * N, None);
        }
        points
            .iter()
            .zip(cells)
            .map(|(&(x, y), ids)| {
                let sum = match usize::try_from(x).ok().zip(usize::try_from(y).ok()).filter(|&(x, y)| x < N && y < N) {
                    Some((x, y)) => *table[x * N + y].get_or_insert_with(|| lights.floor_light(x as i32, y as i32, ids)),
                    None => lights.floor_light(x, y, ids),
                };
                crate::light::vertex_colour(sum, group.0, group.1)
            })
            .collect()
    }
}
