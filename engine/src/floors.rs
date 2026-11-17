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
}

/// The lot's strips, rooms in order: the patterns' runs, then the terrain's, each in row order
/// (the batches' order within a room is not the original's, which groups a room's runs by
/// material: it does not show). `material` gives a shader's diffuse and ambient triples
/// (`Shader::material`); `size` the tiles across the lot to cover (the light grid's points: the
/// original draws no floor past them).
pub fn strips(walls: &Walls, size: i32, patterns: &[FloorPattern], terrain: u32, lights: &LotLights, material: impl Fn(u32) -> Option<([f32; 3], [f32; 3])>) -> Vec<Strip> {
    let runs = floor_runs(walls, size, patterns).into_iter().map(|r| (r, patterns[usize::from(r.value)].shader)).chain(terrain_runs(walls).into_iter().map(|r| (r, terrain)));
    let mut out: Vec<Strip> = runs
        .map(|(run, shader)| {
            let room = lights.room(run.x, run.y);
            let vertices = run.vertices();
            let group = lights.floor_group(room, material(shader).unwrap_or(([1.0; 3], [0.0; 3])));
            let points: Vec<(i32, i32)> = vertices.iter().map(|v| (v.0[0] as i32, v.0[1] as i32)).collect();
            let colours = lights.floor_colours(room, &group, &points);
            let corners = vertices.into_iter().zip(colours).map(|((p, uv), c)| (p, uv, c)).collect();
            Strip { run, room, shader, corners }
        })
        .collect();
    out.sort_by_key(|s| s.room);
    out
}
