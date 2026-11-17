//! House Fire's own Vulkan renderer (raw `ash`), offscreen: meshes and textures uploaded once,
//! a frame of draws rendered into a picture. The simulation crate (`..`) has no GPU
//! dependency; this crate links it only to read the game's models and textures.

pub mod frame;
pub mod gpu;
pub mod renderer;
pub mod scene;
pub mod engine_scene;
pub mod sims;
