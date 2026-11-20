//! `ui-check LOG ISO FRAME...`: plays the Apt player over the observer log to each frame, draws
//! its display list on the GPU over the game's picture of that frame before its 2D layer
//! (`$HF_FRAMES/ui/world_NNNNN.png`), and compares with the game's own picture
//! (`frame_NNNNN.png`) on the 2D layer: the pixels where the game's two pictures differ by more
//! than 4 in a channel. The portrait square (the game's own 3D draw, not the movies') is left
//! out. Writes ours and a difference picture under `$HF_FRAMES/ui-out/`.
//!
//! `HF_UI_SIZE=WxH` draws (and compares) at another size (the reference is scaled by nearest
//! texel: a study aid only). `HF_UI_NEAREST=1` samples the textures at the nearest texel.

use ash::vk;
use housefire::apt::play;
use housefire::disc::Disc;
use housefire::tree_log::read_log;
use housefire_render::apt_draw::{Align, AptAssets, Drawing, Painter, TextRun};
use housefire_render::apt_pass::AptPass;
use housefire_render::gpu::Gpu;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 448;
const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

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

/// Draws a display list's frame on the GPU over `base` (RGBA, `WIDTH` by `HEIGHT`).
fn draw_over(gpu: &Gpu, pass: &mut AptPass, colour: &housefire_render::gpu::Image, base: &[u8], drawing: &Drawing) -> Vec<u8> {
    let d = &gpu.device;
    pass.set(&drawing.vertices, &drawing.batches);
    let staging = gpu.staging(base);
    let out = gpu.buffer(u64::from(WIDTH * HEIGHT * 4), vk::BufferUsageFlags::TRANSFER_DST, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT);
    let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
    let barrier = |from, to, src, dst| vk::ImageMemoryBarrier::default().old_layout(from).new_layout(to).src_access_mask(src).dst_access_mask(dst).image(colour.image).subresource_range(range);
    let region = vk::BufferImageCopy::default().image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1)).image_extent(vk::Extent3D { width: WIDTH, height: HEIGHT, depth: 1 });
    gpu.one_shot(|cb| unsafe {
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_WRITE)]);
        d.cmd_copy_buffer_to_image(cb, staging.buffer, colour.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, vk::AccessFlags::TRANSFER_WRITE, vk::AccessFlags::TRANSFER_READ)]);
        pass.record(cb);
        d.cmd_copy_image_to_buffer(cb, colour.image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, out.buffer, &[region]);
    });
    let rgba = gpu.read(&out, (WIDTH * HEIGHT * 4) as usize);
    gpu.destroy_buffer(staging);
    gpu.destroy_buffer(out);
    rgba
}

/// The words the game draws itself (not in the movies): `$HF_FRAMES/ui/texts.txt`, a line for each
/// run, `FRAME|FONT|X|Y|l, c or r|SCALE|WIDTH|RRGGBBAA|SHADOW|TEXT`. They are the game's own strings,
/// so the file is not in the repository.
fn read_texts(path: &Path) -> Vec<(u32, TextRun)> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(10, '|').collect();
            if f.len() < 10 {
                return None;
            }
            let align = match f[4] {
                "c" => Align::Center,
                "r" => Align::Right,
                _ => Align::Left,
            };
            Some((f[0].parse().ok()?, TextRun { text: f[9].into(), font: f[1].into(), x: f[2].parse().ok()?, y: f[3].parse().ok()?, align, scale: f[5].parse().ok()?, width: f[6].parse().ok()?, color: u32::from_str_radix(f[7], 16).ok()?, shadow: f[8].parse().ok()? }))
        })
        .collect()
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
    let colour = gpu.image(
        &vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(FORMAT)
            .extent(vk::Extent3D { width: WIDTH, height: HEIGHT, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST),
        vk::ImageAspectFlags::COLOR,
    );
    let mut pass = AptPass::new(&gpu, colour.view, FORMAT, WIDTH, HEIGHT, std::env::var_os("HF_UI_NEAREST").is_none());
    let mut painter = Painter::new(&assets);
    let out_dir = std::env::var_os("HF_UI_OUT").map_or_else(|| frames_dir.join("ui-out"), PathBuf::from);
    std::fs::create_dir_all(&out_dir).expect("the output folder");
    let lag: u32 = std::env::var("HF_UI_LAG").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let texts = read_texts(&std::env::var_os("HF_UI_TEXTS").map_or_else(|| frames_dir.join("ui").join("texts.txt"), PathBuf::from));
    let (mut total, mut total_within) = (0usize, 0usize);
    for &frame in &frames {
        let ui = frames_dir.join("ui");
        let (Some(world), Some(game)) = (read_png(&ui.join(format!("world_{frame:05}.png"))), read_png(&ui.join(format!("frame_{frame:05}.png")))) else {
            eprintln!("ui: frame {frame}: no pictures in {}", ui.display());
            return ExitCode::FAILURE;
        };
        let (player, _) = play::run(&log, assets.flashes.clone(), Some(frame.saturating_sub(lag)), play::Timing::default());
        let list = player.display_list();
        let mut drawing = painter.draw(&mut pass, &list, (WIDTH, HEIGHT));
        for run in texts.iter().filter(|(f, _)| *f == frame).map(|(_, r)| r) {
            painter.text(&mut pass, &mut drawing, run);
        }
        let ours = draw_over(&gpu, &mut pass, &colour, &world, &drawing);
        write_png(&out_dir.join(format!("ours_{frame:05}.png")), &ours);
        let masked = |i: usize| {
            let (x, y) = (i % WIDTH as usize, i / WIDTH as usize);
            portrait_box(frame).is_some_and(|b| x >= b[0] && x < b[2] && y >= b[1] && y < b[3])
        };
        let (mut layer, mut within, mut sq) = (0usize, 0usize, 0f64);
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
            if on_layer {
                within += usize::from(worst <= 4);
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
        println!("ui: frame {frame}, layer {layer} pixels, within 4 {within} ({:.2}%), beyond {}, PSNR {psnr:.2} dB", 100.0 * within as f64 / layer.max(1) as f64, layer - within);
        total += layer;
        total_within += within;
    }
    println!("ui: frames {}, layer {total}, within 4 {:.2}%", frames.len(), 100.0 * total_within as f64 / total.max(1) as f64);
    ExitCode::SUCCESS
}
