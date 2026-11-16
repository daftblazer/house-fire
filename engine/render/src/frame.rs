//! What the renderer is asked to draw: a frame is a camera and a list of draws. Everything a
//! draw needs is a handle to something uploaded once (a mesh, textures) or a few numbers (its
//! world matrix, material state and light set); nothing here is per vertex.

/// A 3x4 matrix, row after row (the three rows of x, y, z, each its three columns and the
/// translation).
pub type Mat3x4 = [f32; 12];

pub const IDENTITY: Mat3x4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// `a` after `b`: the matrix that takes a point through `b`, then `a`.
pub fn concat(a: &Mat3x4, b: &Mat3x4) -> Mat3x4 {
    let mut m = [0.0; 12];
    for r in 0..3 {
        for c in 0..4 {
            m[r * 4 + c] = (0..3).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum::<f32>() + if c == 3 { a[r * 4 + 3] } else { 0.0 };
        }
    }
    m
}

/// The camera: a view matrix and a perspective projection in the form the engine keeps it
/// (clip x = p0 x + p1 z, y = p2 y + p3 z, z = p4 z + p5, w = -z; depth 0 at the far plane and
/// -1 at the near one before the renderer's own mapping).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub view: Mat3x4,
    pub projection: [f32; 6],
}

/// A mesh uploaded to the renderer (`Renderer::load_meshes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(pub u32);

/// A texture uploaded to the renderer (`Renderer::load_texture`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);

/// A vertex as meshes keep it: the compact forms of the models (16-bit positions and
/// coordinates, 8-bit normals), scaled in the shader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Vertex {
    /// x, y, z in the model's own units (the draw's matrix carries the scale), and padding.
    pub position: [i16; 4],
    /// Texture coordinates in 4096ths.
    pub uv: [i16; 2],
    pub colour: [u8; 4],
    /// Normal in 128ths, and padding.
    pub normal: [i8; 4],
}

/// A mesh's data, as kept in memory before it is uploaded: indexed triangles.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// How texture coordinates outside 0..1 are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Wrap {
    Clamp,
    Repeat,
    Mirror,
}

/// How a texture's mip levels are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mipmaps {
    Off,
    /// The nearest level.
    Nearest,
    /// Two levels mixed.
    Linear,
}

/// A texture as a layer reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureRef {
    pub texture: TextureId,
    pub wrap: (Wrap, Wrap),
    /// Bilinear filtering (both ways).
    pub linear: bool,
    pub mipmaps: Mipmaps,
}

/// How a material's layers make the pixel (`docs/assets.md`, "The renderer").
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Combine {
    /// The lighting's colour alone.
    Vertex,
    /// Layer 0 times the lighting's colour (or alone when `unlit`), doubled.
    Modulate { unlit: bool },
    /// Each layer times the lighting's colour, mixed by `fade` (the second one's share), doubled.
    Crossfade { fade: f32 },
    /// Layer 1 over layer 0 by layer 0's alpha (`by_texture_alpha`: layer 1's own), doubled;
    /// layer 0 is times the lighting's colour unless `unlit_first`.
    Over { by_texture_alpha: bool, unlit_first: bool },
    /// Layer 1 times layer 0's alpha, added to the picture.
    Add { unlit_first: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    Opaque,
    /// Source alpha over the picture.
    Alpha,
    /// Source alpha times the colour, added.
    Add,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cull {
    None,
    Back,
    Front,
}

/// A material: how the layers combine and the state the draw is made in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub combine: Combine,
    pub layers: [Option<TextureRef>; 2],
    pub blend: Blend,
    pub cull: Cull,
    pub depth_test: bool,
    pub depth_write: bool,
    /// Pixels whose alpha is below this are not drawn.
    pub alpha_ref: Option<f32>,
}

/// A light shining on the draw, in the draw's view space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Directional {
    pub colour: [f32; 3],
    /// The unit vector from the surface towards the light.
    pub to_light: [f32; 3],
}

/// A point light: its colour is divided by `k0 + k1 * distance`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub colour: [f32; 3],
    pub position: [f32; 3],
    pub k0: f32,
    pub k1: f32,
}

pub const MAX_DIRECTIONALS: usize = 2;
pub const MAX_POINTS: usize = 4;

/// What colour a surface gets before its textures.
#[derive(Clone, Debug, PartialEq)]
pub enum Lighting {
    /// Lit by an ambient colour and lights.
    Lit { ambient: [f32; 3], directionals: Vec<Directional>, points: Vec<Point> },
    /// The mesh's own vertex colours.
    VertexColours,
    /// Black at half alpha (what a shadow is drawn with).
    Black,
}

#[derive(Clone, Debug)]
pub struct Draw {
    pub mesh: MeshId,
    pub world: Mat3x4,
    /// The view this draw is made with, where it is not the camera's (the engine sets its own
    /// for some draws).
    pub view: Option<Mat3x4>,
    pub material: Material,
    pub lighting: Lighting,
    /// Written to the id buffer where this draw is the last to cover a pixel.
    pub id: u32,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub camera: Camera,
    pub draws: Vec<Draw>,
}

/// A rendered picture: colour (RGBA, rows from the top) and the id of the last draw to cover
/// each pixel (0: none; else `Draw::id`).
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub ids: Vec<u32>,
}
