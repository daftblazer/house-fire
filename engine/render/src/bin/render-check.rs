//! `render-check LOG ISO FRAME`: renders one frame of the home lot from the observer log's
//! records with the engine's own renderer, and compares it with the recompiled game's own dump
//! of that frame (`$HF_FRAMES/world_NNNNN.png`, the picture before the 2D layer). Writes our
//! picture and a difference picture under `$HF_FRAMES/r1/`.

use housefire::disc::Disc;
use housefire::tree_log::read_log;
use housefire_render::gpu::Gpu;
use housefire_render::renderer::Renderer;
use housefire_render::scene::{Assets, Class};
use std::collections::BTreeMap;
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
        png::ColorType::Rgb => Some(buf.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect()),
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

/// Within-4 share, count beyond and PSNR of the colour channels over the pixels `keep` names.
fn compare(label: &str, ours: &[u8], reference: &[u8], keep: impl Fn(usize) -> bool) {
    let (mut n, mut within, mut sq) = (0u64, 0u64, 0f64);
    for p in (0..(WIDTH * HEIGHT) as usize).filter(|&p| keep(p)) {
        n += 1;
        let d: Vec<i32> = (0..3).map(|c| (i32::from(ours[p * 4 + c]) - i32::from(reference[p * 4 + c])).abs()).collect();
        within += u64::from(d.iter().all(|&x| x <= 4));
        sq += d.iter().map(|&x| f64::from(x * x)).sum::<f64>();
    }
    let psnr = if sq == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64 * 255.0 / (sq / (n * 3) as f64)).log10() };
    println!("{label}: {n} pixels, {:.2}% within 4 in every channel, {} beyond, PSNR {psnr:.2} dB", 100.0 * within as f64 / n.max(1) as f64, n - within);
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(log_path), Some(iso), Some(frame)) = (args.get(1), args.get(2), args.get(3).and_then(|f| f.parse::<u32>().ok())) else {
        eprintln!("usage: render-check LOG ISO FRAME   (HF_FRAMES: the folder of the frame dumps)");
        return ExitCode::from(2);
    };
    let Some(frames_dir) = std::env::var_os("HF_FRAMES").map(PathBuf::from) else {
        eprintln!("set HF_FRAMES to the folder holding world_NNNNN.png");
        return ExitCode::from(2);
    };
    let Some(log) = read_log(log_path) else {
        eprintln!("cannot read the log");
        return ExitCode::FAILURE;
    };
    let disc = Disc::open(iso).expect("the disc image");
    // the frame drawn in a tick: the display copies done before it are the frame's number less 1
    let ticks: std::collections::BTreeSet<u32> = log.strips.iter().map(|s| s.tick).collect();
    let first_tick = *ticks.first().expect("no draw records");
    let first_frame = log.frames.iter().find(|f| f.0 == first_tick).map_or(0, |f| f.1);
    let Some(&tick) = ticks.iter().nth(frame.wrapping_sub(first_frame) as usize) else {
        eprintln!("the log has draws for frames {first_frame}.. ({} of them)", ticks.len());
        return ExitCode::FAILURE;
    };
    let reference = read_png(&frames_dir.join(format!("world_{frame:05}.png"))).expect("the reference picture");

    let assets = Assets::new(&disc);
    let mut scene = assets.scene(&log, tick);

    let gpu = Gpu::new(std::env::var_os("HF_VALIDATE").is_some()).expect("Vulkan");
    let mut renderer = Renderer::new(&gpu, WIDTH, HEIGHT);
    let mesh_ids = renderer.load_meshes(&scene.meshes);
    let texture_ids: Vec<_> = scene.textures.iter().map(|levels| renderer.load_texture(levels)).collect();
    for d in &mut scene.frame.draws {
        d.mesh = mesh_ids[d.mesh.0 as usize];
        for l in d.material.layers.iter_mut().flatten() {
            l.texture = texture_ids[l.texture.0 as usize];
        }
    }
    let picture = renderer.render(&scene.frame);

    // the report: every draw of the frame by class, and the pixels each class covers
    let mut draws: BTreeMap<Class, usize> = BTreeMap::new();
    for d in &scene.all {
        *draws.entry(d.class).or_default() += 1;
    }
    let other_commands = log.other_draws.len() / ticks.len().max(1);
    let mut pixels: BTreeMap<Class, u64> = BTreeMap::new();
    for &id in &picture.ids {
        if id != 0 {
            *pixels.entry(scene.drawn[id as usize - 1].class).or_default() += 1;
        }
    }
    println!("frame {frame} (tick {tick}): {} draws drawn of {} strips; {} other commands ('g' records, about)", scene.drawn.len(), scene.all.len(), other_commands);
    println!("{:<44} {:>7} {:>10}", "class", "draws", "pixels");
    for c in Class::ALL {
        let n = if c == Class::Other { other_commands } else { draws.get(&c).copied().unwrap_or(0) };
        println!("{:<44} {n:>7} {:>10}", c.name(), pixels.get(&c).copied().unwrap_or(0));
    }
    let missing = scene.drawn.iter().filter(|d| d.missing_texture).count();
    let mut unsupported: BTreeMap<&str, usize> = BTreeMap::new();
    for d in scene.drawn.iter().filter_map(|d| d.unsupported) {
        *unsupported.entry(d).or_default() += 1;
    }
    println!("drawn with a texture not found (white): {missing}; materials not drawn right: {unsupported:?}");

    compare("rigid draws' pixels", &picture.rgba, &reference, |p| picture.ids[p] != 0);
    // where the error is: the models whose pixels are furthest off
    let mut by_model: BTreeMap<String, (u64, u64, [i64; 3])> = BTreeMap::new();
    for (p, &id) in picture.ids.iter().enumerate().filter(|x| *x.1 != 0) {
        let info = &scene.drawn[id as usize - 1];
        let off = (0..3).any(|c| (i32::from(picture.rgba[p * 4 + c]) - i32::from(reference[p * 4 + c])).abs() > 4);
        let e = by_model.entry(format!("{} {:?}", info.model, info.class)).or_default();
        e.0 += 1;
        e.1 += u64::from(off);
        for c in 0..3 {
            e.2[c] += i64::from(picture.rgba[p * 4 + c]) - i64::from(reference[p * 4 + c]);
        }
    }
    let mut worst: Vec<_> = by_model.into_iter().collect();
    worst.sort_by_key(|w| std::cmp::Reverse((w.1).1));
    for (name, (n, off, signed)) in worst.iter().take(std::env::var("HF_WORST").ok().and_then(|v| v.parse().ok()).unwrap_or(0)) {
        println!("  {off:>6} of {n:>6} off (mean signed {:+.1} {:+.1} {:+.1}): {name}", signed[0] as f64 / *n as f64, signed[1] as f64 / *n as f64, signed[2] as f64 / *n as f64);
    }
    for probe in std::env::var("HF_PROBE").unwrap_or_default().split(';').filter(|p| !p.is_empty()) {
        let xy: Vec<usize> = probe.split(',').filter_map(|v| v.parse().ok()).collect();
        let p = xy[1] * WIDTH as usize + xy[0];
        let id = picture.ids[p];
        println!("probe {probe}: ours {:?} reference {:?}", &picture.rgba[p * 4..p * 4 + 4], &reference[p * 4..p * 4 + 4]);
        if id != 0 {
            let (info, d) = (&scene.drawn[id as usize - 1], &scene.frame.draws[id as usize - 1]);
            println!("   {info:?}\n   {:?}\n   {:?}", d.material, d.lighting);
        }
    }
    compare("whole frame", &picture.rgba, &reference, |_| true);

    let out = frames_dir.join("r1");
    std::fs::create_dir_all(&out).unwrap();
    write_png(&out.join(format!("ours_{frame}.png")), &picture.rgba.chunks_exact(4).flat_map(|c| [c[0], c[1], c[2], 255]).collect::<Vec<_>>());
    let diff: Vec<u8> = (0..(WIDTH * HEIGHT) as usize)
        .flat_map(|p| {
            let covered = picture.ids[p] != 0;
            let d = |c: usize| (i32::from(picture.rgba[p * 4 + c]) - i32::from(reference[p * 4 + c])).unsigned_abs().saturating_mul(4).min(255) as u8;
            // pixels of the rigid draws in colour, the rest dimmed
            let f = if covered { 1 } else { 4 };
            [d(0) / f, d(1) / f, d(2) / f, 255]
        })
        .collect();
    write_png(&out.join(format!("diff_{frame}.png")), &diff);
    ExitCode::SUCCESS
}
