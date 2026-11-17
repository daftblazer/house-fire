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

/// The first id of the transient meshes (`Renderer::set_transient_meshes`): their ids are this
/// bit and their place in the list given, valid for one frame.
pub const TRANSIENT_MESH: u32 = 1 << 31;

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

/// A skinned vertex's blend: the four bones' weight bytes (of 255) and the bone that turns its
/// normal, the first of the largest weights (carried from the vertex before when a vertex has
/// none; `housefire::skin`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkinWeights {
    pub weights: [u8; 4],
    pub heaviest: u8,
}

/// A mesh's data, as kept in memory before it is uploaded: indexed triangles.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// The blend of each vertex of a skinned mesh (its positions are then the vertices' own,
    /// blended on the GPU through a draw's bones); empty for a rigid mesh.
    pub skin: Vec<SkinWeights>,
    /// The texture coordinates are 4096ths of `1 << uv_shift` (a floor's run of tiles reaches
    /// past 8, where the 16-bit range ends at shift 0).
    pub uv_shift: u8,
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

/// How a draw's colour meets the picture: GX's blend modes (GXSetBlendMode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    Opaque,
    /// source x `src` + picture x `dst`.
    Mix { src: Factor, dst: Factor },
    /// picture - source.
    Subtract,
}

impl Blend {
    /// Source alpha over the picture.
    pub const ALPHA: Blend = Blend::Mix { src: Factor::SrcAlpha, dst: Factor::InvSrcAlpha };
    /// Source alpha times the colour, added.
    pub const ADD: Blend = Blend::Mix { src: Factor::SrcAlpha, dst: Factor::One };
}

/// A blend factor, as GX numbers them (0 to 7). `Other` is the other side's colour (the
/// picture's for the source factor, the source's for the picture's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Factor {
    Zero,
    One,
    Other,
    InvOther,
    SrcAlpha,
    InvSrcAlpha,
    DstAlpha,
    InvDstAlpha,
}

impl Factor {
    pub fn from_gx(code: u32) -> Factor {
        [Factor::Zero, Factor::One, Factor::Other, Factor::InvOther, Factor::SrcAlpha, Factor::InvSrcAlpha, Factor::DstAlpha, Factor::InvDstAlpha][code as usize & 7]
    }
}

/// A compare function, as GX (and Vulkan) number them: never, less, equal, less or equal,
/// greater, not equal, greater or equal, always.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Compare(pub u8);

impl Compare {
    pub const LEQUAL: Compare = Compare(3);
    pub const GEQUAL: Compare = Compare(6);
    pub const ALWAYS: Compare = Compare(7);
}

/// GX's alpha compare: a pixel is drawn when (alpha `comp[0]` `refs[0]`) `op` (alpha
/// `comp[1]` `refs[1]`), the alpha and references in 8 bits; `op` is 0 and, 1 or, 2 xor,
/// 3 xnor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AlphaTest {
    pub comp: [Compare; 2],
    pub refs: [u8; 2],
    pub op: u8,
}

impl AlphaTest {
    /// Alpha at least `r` (of 255).
    pub fn at_least(r: u8) -> AlphaTest {
        AlphaTest { comp: [Compare::GEQUAL, Compare::ALWAYS], refs: [r, 0], op: 0 }
    }
    /// Whether it lets every pixel through.
    pub fn passes_all(&self) -> bool {
        let all = |c: Compare| c == Compare::ALWAYS;
        match self.op {
            0 => all(self.comp[0]) && all(self.comp[1]),
            1 => all(self.comp[0]) || all(self.comp[1]),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cull {
    None,
    Back,
    Front,
    /// Every triangle (GX_CULL_ALL): nothing is drawn.
    All,
}

/// How much of the sphere-mapped layer is added.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SphereWeight {
    One,
    /// By the colour's alpha so far.
    PrevAlpha,
    Factor(f32),
}

/// A material: how the layers combine and the state the draw is made in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub combine: Combine,
    pub layers: [Option<TextureRef>; 2],
    pub blend: Blend,
    pub cull: Cull,
    /// The depth test's function (none: not tested, and not written either, as on GX).
    pub depth_test: Option<Compare>,
    pub depth_write: bool,
    /// Which pixels are drawn by their alpha (none: all).
    pub alpha_test: Option<AlphaTest>,
    /// Which of the picture's channels the draw writes (GXSetColorUpdate, GXSetAlphaUpdate).
    pub write_colour: bool,
    pub write_alpha: bool,
    /// A constant written as the destination alpha instead of the draw's own (GXSetDstAlpha):
    /// the picture keeps an alpha that the shadows' blend reads back.
    pub dst_alpha: Option<u8>,
    /// Layer 1 is read at the vertex position's x and y (in mesh units) instead of at the
    /// vertex's own coordinates: the terrain's lot-wide colour texture.
    pub layer1_from_position: Option<PositionUv>,
    /// A reflection added over the colour: layer 1 read through the sphere map of the surface
    /// (`Combine::Modulate` only).
    pub sphere: Option<SphereWeight>,
}

/// A texture coordinate made from a vertex position (a texture coordinate generator): each of
/// u and v is a row (x factor, y factor, offset) applied to the position in the mesh's units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionUv {
    pub u: [f32; 3],
    pub v: [f32; 3],
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

/// A rectangle of the picture, in pixels from the top left (GXSetViewport's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    /// The whole pixels the viewport touches (the scissor): left, top, width, height.
    pub fn pixels(&self) -> (i32, i32, u32, u32) {
        let (x0, y0) = (self.x.floor(), self.y.floor());
        (x0 as i32, y0 as i32, ((self.x + self.width).ceil() - x0) as u32, ((self.y + self.height).ceil() - y0) as u32)
    }
}

#[derive(Clone, Debug)]
pub struct Draw {
    pub mesh: MeshId,
    /// The matrix from the mesh's units (for a skinned mesh: from the blended positions) to the
    /// world.
    pub world: Mat3x4,
    /// A skinned mesh's bones: each a 3x4 matrix from the mesh's units to the world, the
    /// vertices' weights choosing among them (`MeshData::skin`).
    pub bones: Option<Box<[Mat3x4; 4]>>,
    /// A projection other than the camera's, drawn inside `viewport` (the interface's draws).
    pub projection: Option<[f32; 6]>,
    pub viewport: Option<Viewport>,
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
    /// A rectangle filled with a colour (red, green, blue) before the first draw with a
    /// viewport (the portrait's backdrop, under the Sim's head).
    pub backdrop: Option<(Viewport, [u8; 3])>,
}

/// A rendered picture: colour (RGBA, rows from the top) and the id of the last draw to cover
/// each pixel (0: none; else `Draw::id`).
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub ids: Vec<u32>,
    /// The id of the last draw to cover each pixel among the ones with the depth test off (the
    /// shadows' passes), which write no id above.
    pub late_ids: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_applies_the_second_matrix_first() {
        // b moves by (1, 0, 0); a doubles
        let b = [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let a = [2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let m = concat(&a, &b);
        assert_eq!([m[0], m[3], m[5], m[10]], [2.0, 2.0, 2.0, 2.0]);
        assert_eq!(concat(&IDENTITY, &b), b);
    }
}
