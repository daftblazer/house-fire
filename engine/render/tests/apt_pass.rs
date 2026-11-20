//! The movies' pass on the GPU: solid and textured triangles, the colour transform, alpha and
//! premultiplied textures, drawn over a small picture and read back. They skip (and pass) where
//! no Vulkan device is there. The last test runs `ui-check` against the game's own frames when
//! HF_ISO, HF_UI_LOG and HF_FRAMES name the disc, the observer log and the frames' folder.

use housefire_render::apt_pass::{self, AptPass, Batch, TextureId, Vertex};
use housefire_render::gpu::Gpu;
use std::process::Command;

const W: u32 = 4;
const H: u32 = 2;

/// A quad over the whole picture (clip space), as two triangles.
fn quad(tint: [f32; 4], mul: [f32; 4], add: [f32; 4], premultiplied: f32) -> Vec<Vertex> {
    let v = |x: f32, y: f32, u: f32, w: f32| Vertex { pos: [x, y], uv: [u, w], tint, mul, add, premultiplied, _pad: 0.0 };
    let (a, b, c, d) = (v(-1.0, -1.0, 0.0, 0.0), v(1.0, -1.0, 1.0, 0.0), v(-1.0, 1.0, 0.0, 1.0), v(1.0, 1.0, 1.0, 1.0));
    vec![a, b, c, b, d, c]
}

/// The picture after drawing `vertices` (with `texture` of `size`, RGBA, or a white texel) over a
/// grey one; `None` without a GPU.
fn draw(vertices: &[Vertex], texture: Option<(u32, u32, &[u8])>, linear: bool) -> Option<Vec<u8>> {
    let gpu = Gpu::new(false).ok()?;
    let target = apt_pass::target(&gpu, W, H);
    let mut pass = AptPass::new(&gpu, target.view, apt_pass::TARGET_FORMAT, W, H);
    let id: TextureId = match texture {
        Some((w, h, rgba)) => pass.add_texture(w, h, rgba, linear),
        None => pass.add_texture(1, 1, &[255; 4], linear),
    };
    pass.set(vertices, &[Batch { texture: id, first: 0, count: vertices.len() as u32 }]);
    let base: Vec<u8> = (0..W * H).flat_map(|_| [100, 100, 100, 255]).collect();
    let out = apt_pass::render_over(&gpu, &pass, &target, &base, W, H);
    drop(pass);
    gpu.destroy_image(target);
    Some(out)
}

fn close(got: u8, want: f32) -> bool {
    (f32::from(got) - want).abs() <= 1.5
}

#[test]
fn a_solid_fill_goes_through_the_colour_transform() {
    // red 1, green 0 and blue 0 tinted; the multiplier halves red, the offset adds 0.2 green
    let v = quad([1.0, 0.0, 0.0, 1.0], [0.5, 1.0, 1.0, 1.0], [0.0, 0.2, 0.0, 0.0], 0.0);
    let Some(px) = draw(&v, None, true) else { return eprintln!("skipped: no Vulkan device") };
    for p in px.chunks(4) {
        assert!(close(p[0], 127.5) && close(p[1], 51.0) && close(p[2], 0.0) && p[3] == 255, "{p:?}");
    }
}

#[test]
fn alpha_blends_over_the_picture() {
    let v = quad([1.0, 1.0, 1.0, 0.5], [1.0; 4], [0.0; 4], 0.0);
    let Some(px) = draw(&v, None, true) else { return eprintln!("skipped: no Vulkan device") };
    // half white over 100 grey
    assert!(close(px[0], 0.5 * 255.0 + 0.5 * 100.0) && px[3] == 255, "{px:?}");
}

#[test]
fn a_texture_is_sampled_by_its_coordinates() {
    // two texels, red and green, over a picture 4 wide: the left half red, the right green
    let tex = [255, 0, 0, 255, 0, 255, 0, 255];
    let v = quad([1.0; 4], [1.0; 4], [0.0; 4], 0.0);
    let Some(px) = draw(&v, Some((2, 1, &tex)), false) else { return eprintln!("skipped: no Vulkan device") };
    assert_eq!(&px[0..4], &[255, 0, 0, 255]);
    assert_eq!(&px[4..8], &[255, 0, 0, 255]);
    assert_eq!(&px[8..12], &[0, 255, 0, 255]);
    assert_eq!(&px[12..16], &[0, 255, 0, 255]);
}

#[test]
fn a_premultiplied_texel_is_divided_by_its_alpha() {
    // half-covered white stored as (128, 128, 128, 128): white at half alpha over 100 grey
    let tex = [128, 128, 128, 128];
    let v = quad([1.0; 4], [1.0; 4], [0.0; 4], 1.0);
    let Some(px) = draw(&v, Some((1, 1, &tex)), true) else { return eprintln!("skipped: no Vulkan device") };
    assert!(close(px[0], 128.0 / 255.0 * 255.0 + (1.0 - 128.0 / 255.0) * 100.0), "{px:?}");
}

#[test]
fn ui_check_holds_its_figure_on_the_games_frames() {
    let get = |k: &str| std::env::var(k).ok();
    let (Some(iso), Some(log), Some(_)) = (get("HF_ISO"), get("HF_UI_LOG"), get("HF_FRAMES")) else {
        return eprintln!("skipped: HF_ISO, HF_UI_LOG and HF_FRAMES name the disc, the observer log and the frames' folder");
    };
    let out = Command::new(env!("CARGO_BIN_EXE_ui-check")).args([&log, &iso, "930", "1194"]).output().expect("ui-check runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let total = text.lines().find(|l| l.starts_with("ui: frames")).unwrap_or_else(|| panic!("no total line in {text}"));
    let pct: f64 = total.rsplit("within 4 ").next().and_then(|t| t.trim_end_matches('%').parse().ok()).unwrap_or(0.0);
    assert!(pct >= 85.0, "{total}");
}
