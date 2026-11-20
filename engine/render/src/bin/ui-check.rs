//! `ui-check LOG ISO FRAME...`: plays the Apt player over the observer log to each frame, draws
//! its display list on the GPU over the game's picture of that frame before its 2D layer
//! (`$HF_FRAMES/ui/world_NNNNN.png`), and compares with the game's own picture
//! (`frame_NNNNN.png`) on the 2D layer: the pixels where the game's two pictures differ by more
//! than 4 in a channel. The portrait square (the game's own 3D draw, not the movies') is left
//! out. Writes ours and a difference picture under `$HF_FRAMES/ui-out/`.
//!

use housefire::apt::play;
use housefire::disc::Disc;
use housefire::tree_log::read_log;
use housefire_render::apt_draw::{AptAssets, Painter};
use housefire_render::game_text;
use housefire_render::apt_pass::{self, AptPass};
use housefire_render::gpu::Gpu;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 448;

fn read_png(path: &Path) -> Option<Vec<u8>> {
    let mut reader = png::Decoder::new(std::fs::File::open(path).ok()?).read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    assert_eq!((info.width, info.height), (WIDTH, HEIGHT), "the reference is {WIDTH}x{HEIGHT}");
    match info.color_type {
        png::ColorType::Rgba => Some(buf),
        png::ColorType::Rgb => Some(buf.as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect()),
        _ => None,
    }
}

fn write_png(path: &Path, rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// The game's own 3D draw inside the HUD's frame, by frame: left, top, right, bottom (pixels).
/// Measured from the pictures (the square the HUD frames, 54 by 56 in them).
fn portrait_box(frame: u32) -> Option<[usize; 4]> {
    (frame >= 1300).then_some([48, 345, 106, 405])
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let frames: Vec<u32> = args.iter().skip(3).filter_map(|a| a.parse().ok()).collect();
    if args.len() < 4 || frames.is_empty() {
        eprintln!("usage: ui-check LOG ISO FRAME...   (HF_FRAMES: the folder holding ui/frame_NNNNN.png and ui/world_NNNNN.png)");
        return ExitCode::from(2);
    }
    let Some(frames_dir) = std::env::var_os("HF_FRAMES").map(PathBuf::from) else {
        eprintln!("set HF_FRAMES to the folder holding ui/");
        return ExitCode::from(2);
    };
    let Some(log) = read_log(&args[1]) else {
        eprintln!("cannot read the log");
        return ExitCode::FAILURE;
    };
    let disc = Disc::open(&args[2]).expect("the disc image");
    let assets = AptAssets::new(&disc);
    let gpu = Gpu::new(std::env::var_os("HF_VALIDATE").is_some()).expect("Vulkan");
    let colour = apt_pass::target(&gpu, WIDTH, HEIGHT);
    let mut pass = AptPass::new(&gpu, colour.view, apt_pass::TARGET_FORMAT, WIDTH, HEIGHT);
    let mut painter = Painter::new(&assets);
    let out_dir = std::env::var_os("HF_UI_OUT").map_or_else(|| frames_dir.join("ui-out"), PathBuf::from);
    std::fs::create_dir_all(&out_dir).expect("the output folder");
    let lag: u32 = std::env::var("HF_UI_LAG").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let (mut total, mut total_within, mut total_over, mut scored) = (0usize, 0usize, 0usize, 0usize);
    for &frame in &frames {
        let ui = frames_dir.join("ui");
        let (Some(world), Some(game)) = (read_png(&ui.join(format!("world_{frame:05}.png"))), read_png(&ui.join(format!("frame_{frame:05}.png")))) else {
            eprintln!("ui: frame {frame}: no pictures in {}", ui.display());
            return ExitCode::FAILURE;
        };
        // a world picture that is all black (frames 760 and 1390 of the dump) has no picture to
        // lay the layer over: the 2D layer is then the whole frame, not the movies' alone
        let black = world.chunks(4).all(|p| p[..3].iter().all(|&c| c < 8));
        let (player, _) = play::run(&log, assets.flashes.clone(), Some(frame.saturating_sub(lag)), play::Timing::default());
        let list = player.display_list();
        let mut drawing = painter.draw(&mut pass, &list);
        for run in &game_text::lines(&assets, &log.text_draws, frame.saturating_sub(lag)) {
            painter.text(&mut pass, &mut drawing, run);
        }
        pass.set(&drawing.vertices, &drawing.batches);
        let ours = apt_pass::render_over(&gpu, &pass, &colour, &world, WIDTH, HEIGHT);
        write_png(&out_dir.join(format!("ours_{frame:05}.png")), &ours);
        let masked = |i: usize| {
            let (x, y) = (i % WIDTH as usize, i / WIDTH as usize);
            portrait_box(frame).is_some_and(|b| x >= b[0] && x < b[2] && y >= b[1] && y < b[3])
        };
        let (mut layer, mut within, mut over, mut sq) = (0usize, 0usize, 0usize, 0f64);
        let mut hist = [0usize; 9];
        let mut diff = vec![0u8; ours.len()];
        for i in 0..(WIDTH * HEIGHT) as usize {
            let p = i * 4;
            let on_layer = (0..3).any(|c| game[p + c].abs_diff(world[p + c]) > 4);
            let worst = (0..3).map(|c| ours[p + c].abs_diff(game[p + c])).max().unwrap();
            diff[p + 3] = 255;
            if masked(i) {
                continue;
            }
            // our picture's difference from the game's, anywhere it could matter: the layer's
            // pixels and the ones we drew over
            let drawn = (0..3).any(|c| ours[p + c].abs_diff(world[p + c]) > 4);
            if !on_layer && !drawn {
                continue;
            }
            layer += usize::from(on_layer);
            over += usize::from(!on_layer);
            if on_layer {
                within += usize::from(worst <= 4);
                hist[(32 - u32::from(worst).leading_zeros()) as usize] += 1;
                for c in 0..3 {
                    let e = f64::from(ours[p + c]) - f64::from(game[p + c]);
                    sq += e * e;
                }
            }
            let v = (worst as u32 * 4).min(255) as u8;
            diff[p] = v;
            diff[p + 1] = if worst > 4 { 0 } else { v };
            diff[p + 2] = if worst > 4 { 0 } else { v };
        }
        write_png(&out_dir.join(format!("diff_{frame:05}.png")), &diff);
        let mse = sq / (layer.max(1) * 3) as f64;
        let psnr = if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() };
        let note = if black { " (world picture black: not scored)" } else { "" };
        println!("ui: frame {frame}, layer {layer} pixels, within 4 {within} ({:.2}%), beyond {}, over {over}, PSNR {psnr:.2} dB{note}", 100.0 * within as f64 / layer.max(1) as f64, layer - within);
        if std::env::var_os("HF_UI_HIST").is_some() {
            println!("    worst channel error, pixels by error bit length (0, 1, 2-3, 4-7, 8-15, 16-31, 32-63, 64-127, 128+): {hist:?}");
        }
        if !black {
            total += layer;
            total_within += within;
            total_over += over;
            scored += 1;
        }
    }
    println!("ui: frames {scored}, layer {total}, within 4 {:.2}%, over {total_over}", 100.0 * total_within as f64 / total.max(1) as f64);
    ExitCode::SUCCESS
}
