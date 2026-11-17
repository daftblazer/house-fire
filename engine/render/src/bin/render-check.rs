//! `render-check LOG ISO FRAME`: renders one frame of the home lot from the observer log's
//! records with the engine's own renderer, and compares it with the recompiled game's own dump
//! of that frame (`$HF_FRAMES/world_NNNNN.png`, the picture before the 2D layer). Writes our
//! picture and a difference picture under `$HF_FRAMES/r1/`.
//!
//! Study aids (environment): `HF_WORST=n` lists the n models whose pixels are furthest off,
//! `HF_PROBE="x,y;x,y"` explains the draw at those pixels, `HF_MODEL=name` samples a model's
//! off pixels, `HF_FLAGS=1` counts the materials' flag words, `HF_EXCLUDE` leaves
//! boxes out of a second figure, `HF_VALIDATE=1` turns the Vulkan validation layer on.

use housefire::disc::Disc;
use housefire::tree_log::read_log;
use housefire_render::gpu::Gpu;
use housefire_render::renderer::Renderer;
use housefire_render::engine_scene;
use housefire_render::frame::{self, Lighting};
use housefire_render::scene::{Assets, Class};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 448;

fn fnv(v: &[u32]) -> u64 {
    v.iter().fold(1469598103934665603u64, |h, &x| (h ^ u64::from(x)).wrapping_mul(1099511628211))
}

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

/// The GPU-skinned positions of every skinned draw against the reference skinner (`housefire::skin`),
/// from each '$' record's bones and the '+' record's inputs; one line.
fn skin_check(log: &housefire::tree_log::Log, tick: u32, scene: &housefire_render::scene::Scene, renderer: &Renderer) {
    let gpu = renderer.read_skinned();
    let wanted: Vec<usize> = (0..log.skin_vertices.len()).filter(|&i| log.skins.get(log.skin_vertices[i].skin).is_some_and(|k| k.tick == tick)).collect();
    let (mut primitives, mut vertices, mut worst, mut worst_normal) = (0usize, 0usize, 0f32, 0f32);
    let mut compared = std::collections::BTreeSet::new();
    let mut over = Vec::new();
    for g in &gpu {
        let Some(skin) = scene.drawn[scene.frame.draws[g.draw].id as usize - 1].skin else { continue };
        let Some(v) = log.skin_vertices.iter().find(|v| v.skin == skin) else { continue };
        let reference = housefire::skin::skin(&log.skins[skin].bones, v.heaviest as usize, v.count as usize, &v.positions, &v.weights, &v.normals);
        if reference.len() != g.positions.len() {
            over.push(format!("draw {} (strip {}): {} vertices on the GPU, {} logged", g.draw, scene.drawn[scene.frame.draws[g.draw].id as usize - 1].strip, g.positions.len(), reference.len()));
            continue;
        }
        let mut here = 0f32;
        for ((r, p), n) in reference.iter().zip(&g.positions).zip(&g.normals) {
            here = here.max((0..3).map(|c| (r.position[c] - p[c]).abs()).fold(0.0, f32::max));
            if let Some(rn) = r.normal {
                let len = rn.iter().map(|&b| f32::from(b) * f32::from(b)).sum::<f32>().sqrt().max(1.0);
                worst_normal = worst_normal.max((0..3).map(|c| (f32::from(rn[c]) / len - n[c]).abs()).fold(0.0, f32::max));
            }
        }
        if here > 1e-3 {
            over.push(format!("draw {} (strip {}): max error {here}", g.draw, scene.drawn[scene.frame.draws[g.draw].id as usize - 1].strip));
        }
        worst = worst.max(here);
        primitives += 1;
        vertices += reference.len();
        compared.insert(skin);
    }
    let missing = wanted.iter().filter(|&&i| !compared.contains(&log.skin_vertices[i].skin)).count();
    println!("GPU skinning against the reference skinner: {primitives} of {} logged primitives compared ({missing} not drawn: no part found), {vertices} vertices, max abs position error {worst:.2e} lot units, max normal component error {worst_normal:.3} (against the unit of the quantised bytes); {} over 1e-3", wanted.len(), over.len());
    for line in over.iter().take(10) {
        println!("  {line}");
    }
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
    // the engine mode (HF_ENGINE=1, with --card CARD for a save's lot): the lot's model draws from
    // the engine's own state in place of the log's rigid and display-list draws
    let engine_mode = std::env::var_os("HF_ENGINE").is_some();
    if engine_mode {
        let card = args.iter().position(|a| a == "--card").and_then(|i| args.get(i + 1)).map(PathBuf::from);
        let view = scene.frame.camera.view;
        let house = std::env::var("HF_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let plan = engine_scene::plan(&disc, card.as_deref(), &log, tick, &view, house).expect("the engine's lot");
        let shaders = housefire::shader::index(&disc);
        if let Ok(name) = std::env::var("HF_DRAWS") {
            for d in &plan.draws {
                let part = plan.models.parts.parts_of(d.part.0).find(|p| p.index == d.part).unwrap();
                if part.record.name.starts_with(&name) {
                    println!("  planned {} {:?} sub {} at {:?} (object {:x?}), {} vertices, shader {:08x}", part.record.name, d.kind, d.part.2, d.origin, (d.object, plan.guids.get(&d.object)), part.batch.count, part.batch.hash);
                }
            }
        }
        // (HF_ORACLE=1, a study aid: the light sets are the log's, of the log draw at the same
        // world matrix, to tell the lights' error from the materials')
        let oracle: Vec<([f32; 12], Lighting, frame::Material, usize)> = if std::env::var_os("HF_ORACLE").is_some() || std::env::var_os("HF_MATDIFF").is_some() {
            scene.frame.draws.iter().filter(|d| matches!(scene.drawn[d.id as usize - 1].class, Class::RigidStrip | Class::DisplayList)).map(|d| (d.world, d.lighting.clone(), d.material, scene.meshes[d.mesh.0 as usize].vertices.len())).collect()
        } else {
            Vec::new()
        };
        let lighting = |d: &housefire::drawlist::Draw| {
            if std::env::var_os("HF_ORACLE").is_none() {
                return engine_scene::lighting_for(&plan.lights.of(d), &view);
            }
            let near = |a: &[f32; 12]| (0..12).all(|i| (a[i] - d.world[i]).abs() <= 1e-4 * if i % 4 == 3 { 1.0 } else { a[0].abs().max(a[5].abs()).max(a[10].abs()).max(1e-9) });
            match oracle.iter().find(|o| near(&o.0)) {
                Some(o) => o.1.clone(),
                None => Lighting::Lit { ambient: [0.5; 3], directionals: Vec::new(), points: Vec::new() },
            }
        };
        let made = assets.engine_draws(&mut scene, &engine_scene::Inputs { draws: &plan.draws, models: &plan.models, shaders: &shaders, lighting: &lighting, view });
        let planned = made.len();
        if std::env::var_os("HF_LIGHTSTUDY").is_some() {
            // the log's light sets in world space, once for each object place; the engine's lamps
            let v = view;
            let to_world = |p: [f32; 3]| -> [f32; 3] { let q = [p[0] - v[3], p[1] - v[7], p[2] - v[11]]; [0, 1, 2].map(|c| (0..3).map(|r| v[4 * r + c] * q[r]).sum::<f32>()) };
            let turn_world = |p: [f32; 3]| -> [f32; 3] { [0, 1, 2].map(|c| (0..3).map(|r| v[4 * r + c] * p[r]).sum::<f32>()) };
            let mut seen = std::collections::BTreeSet::new();
            for m in &made {
                let Some(o) = oracle.iter().find(|o| (0..12).all(|i| (o.0[i] - m.draw.world[i]).abs() < 1e-4 * if i % 4 == 3 { 1.0 } else { 0.05 })) else { continue };
                let key = format!("{:?}", o.1);
                if !seen.insert(key) { continue; }
                let Lighting::Lit { ambient, directionals, points } = &o.1 else { println!("LS {} vertex colours", m.info.model); continue };
                let org = plan.draws.iter().zip(&made).find(|(_, mm)| std::ptr::eq(*mm, m)).map(|(d, _)| (d.origin, d.object));
                let dirs: Vec<String> = directionals.iter().map(|d| format!("dir {:.3?} to {:.3?}", d.colour, turn_world(d.to_light))).collect();
                let pts: Vec<String> = points.iter().map(|p| format!("pt {:.2?} col {:.3?} k1 {}", to_world(p.position), p.colour, p.k1)).collect();
                println!("LS {} at {:?} amb {:.3?} {} {}", m.info.model, org, ambient, dirs.join(" "), pts.join(" "));
            }
            println!("LS clock {:?}; the engine's sun {:?}", plan.clock, plan.lights.at(1.0, 1.0).sun);
            let mut shown = std::collections::BTreeSet::new();
            for (d, m) in plan.draws.iter().zip(&made) {
                if !shown.insert((d.origin[0].to_bits(), d.origin[1].to_bits())) { continue; }
                let Lighting::Lit { ambient, directionals, points } = engine_scene::lighting_for(&plan.lights.of(d), &view) else { continue };
                let dirs: Vec<String> = directionals.iter().map(|d| format!("dir {:.3?} to {:.3?}", d.colour, turn_world(d.to_light))).collect();
                let pts: Vec<String> = points.iter().map(|p| format!("pt {:.2?} col {:.3?} k1 {}", to_world(p.position), p.colour, p.k1)).collect();
                println!("LE {} at {:?} amb {:.3?} {} {}", m.info.model, d.origin, ambient, dirs.join(" "), pts.join(" "));
            }
            for l in &plan.lamps {
                for x in &l.lights {
                    println!("LAMP leader {} at ({}, {}) light node {} at {:.2?} colour {:.3?} reach {}", l.leader, l.place.x, l.place.y, x.node, x.at, x.colour, x.reach);
                }
            }
        }
        if std::env::var_os("HF_MATDIFF").is_some() {
            // the materials against the log's, by world matrix and vertex count: what differs, by model
            let near = |a: &[f32; 12], b: &[f32; 12]| (0..12).all(|i| (a[i] - b[i]).abs() <= 1e-4 * if i % 4 == 3 { 1.0 } else { a[0].abs().max(a[5]).max(a[10]).max(1e-9) });
            let tex = |scene: &housefire_render::scene::Scene, m: &frame::Material, k: usize| m.layers[k].map(|l| (scene.textures.get(l.texture.0 as usize).map(|t| (t[0].0, t[0].1, t.len(), fnv(&t[0].2))), l.wrap, l.linear, l.mipmaps));
            let mut by: BTreeMap<String, (usize, String)> = BTreeMap::new();
            for m in &made {
                let n = scene.meshes[m.draw.mesh.0 as usize].vertices.len();
                let Some(o) = oracle.iter().find(|o| o.3 == n && near(&o.0, &m.draw.world)) else {
                    let s = housefire::scene::Camera::on_screen(&view, &scene.frame.camera.projection, [m.draw.world[3], m.draw.world[7], m.draw.world[11]]);
                    if s.is_some_and(|s| (0.0..=1.04).contains(&s[0]) && (-0.19..=0.63).contains(&s[1])) {
                        by.entry(format!("no log draw, on screen, {}", m.info.model)).or_insert((0, format!("{:?}", s))).0 += 1;
                    }
                    by.entry("no log draw".into()).or_insert((0, m.info.model.clone())).0 += 1;
                    continue;
                };
                let (e, l) = (&m.draw.material, &o.2);
                let mut diffs = Vec::new();
                if std::mem::discriminant(&e.combine) != std::mem::discriminant(&l.combine) || e.combine != l.combine && !matches!(e.combine, frame::Combine::Crossfade { .. }) {
                    diffs.push(format!("combine {:?} vs {:?}", e.combine, l.combine));
                }
                if e.blend != l.blend {
                    diffs.push(format!("blend {:?} vs {:?}", e.blend, l.blend));
                }
                if e.alpha_test != l.alpha_test {
                    diffs.push(format!("alpha {:?} vs {:?}", e.alpha_test.map(|a| a.refs[0]), l.alpha_test.map(|a| a.refs[0])));
                }
                if e.depth_write != l.depth_write || e.depth_test != l.depth_test {
                    diffs.push("depth".into());
                }
                if e.dst_alpha != l.dst_alpha || e.write_alpha != l.write_alpha || e.write_colour != l.write_colour {
                    diffs.push(format!("dstalpha {:?}/{} vs {:?}/{}", e.dst_alpha, e.write_alpha, l.dst_alpha, l.write_alpha));
                }
                for k in 0..2 {
                    let (te, tl) = (tex(&scene, e, k), tex(&scene, l, k));
                    if te.as_ref().map(|t| &t.0) != tl.as_ref().map(|t| &t.0) {
                        diffs.push(format!("layer {k} texture {:?} vs {:?}", te.as_ref().map(|t| (t.0.map(|x| (x.0, x.1, x.2)), &m.info.textures[k])), tl.as_ref().map(|t| t.0.map(|x| (x.0, x.1, x.2)))));
                    } else if te != tl {
                        diffs.push(format!("layer {k} sampler {:?} vs {:?}", te.map(|t| (t.1, t.2, t.3)), tl.map(|t| (t.1, t.2, t.3))));
                    }
                }
                // the log's state by what the engine knows of the draw: its kind, blend, the layer 0
                // texture's alpha (none, some pixels clear, all clear)
                let alpha = m.draw.material.layers[0].and_then(|l| scene.textures.get(l.texture.0 as usize)).map_or("no texture", |t| {
                    let n = t[0].2.iter().filter(|&&p| p >> 24 != 255).count();
                    if n == 0 { "opaque" } else if n == t[0].2.len() { "all clear" } else { "cutout" }
                });
                let key = format!("STATE {:?} {:?} layers {} texture {alpha}: log alpha {:?} dst {:?}/{} depth {:?}/{}", m.info.class, e.blend, e.layers.iter().flatten().count(), l.alpha_test.map(|a| (a.refs[0], a.comp[0].0, a.op)), l.dst_alpha, l.write_alpha, l.depth_test.map(|c| c.0), l.depth_write);
                by.entry(key).or_insert((0, m.info.model.clone())).0 += 1;
                by.entry(if diffs.is_empty() { "same".into() } else { diffs.join("; ") }).or_insert((0, m.info.model.clone())).0 += 1;
            }
            for (k, (n, ex)) in by {
                println!("MAT {n:>4} e.g. {ex}: {k}");
            }
        }
        let removed = engine_scene::splice(&mut scene, made);
        println!("engine mode: {} draws planned, {planned} made; the log's {removed} rigid and display-list draws left out", plan.draws.len());
    }

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

    skin_check(&log, tick, &scene, &renderer);

    // the report: every draw of the frame by class, and the pixels each class covers
    let mut draws: BTreeMap<Class, usize> = BTreeMap::new();
    for d in &scene.all {
        *draws.entry(d.class).or_default() += 1;
    }
    let other_commands = log.other_draws.iter().filter(|g| g.0.tick == tick).count();
    let mut pixels: BTreeMap<Class, u64> = BTreeMap::new();
    for &id in picture.ids.iter().chain(&picture.late_ids) {
        if id != 0 {
            *pixels.entry(scene.drawn[id as usize - 1].class).or_default() += 1;
        }
    }
    println!("frame {frame} (tick {tick}): {} draws drawn of {} strips; {} other commands ('g' records)", scene.drawn.len(), scene.all.len(), other_commands);
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
    let missing_pixels = picture.ids.iter().filter(|&&id| id != 0 && scene.drawn[id as usize - 1].missing_texture).count();
    println!("drawn with a texture not found (white): {missing} draws, {missing_pixels} pixels; materials not drawn right: {unsupported:?}");

    if std::env::var_os("HF_LIGHTS").is_some() {
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for d in &scene.frame.draws {
            if matches!(scene.drawn[d.id as usize - 1].class, Class::RigidStrip | Class::DisplayList) {
                *by.entry(format!("{:?}", d.lighting)).or_default() += 1;
            }
        }
        for (k, v) in by {
            println!("LIGHT {v}: {k}");
        }
    }
    let class_at = |id: u32| scene.drawn[id as usize - 1].class;
    let rigid = |p: usize| picture.ids[p] != 0 && matches!(class_at(picture.ids[p]), Class::RigidStrip | Class::DisplayList);
    compare("rigid draws' pixels", &picture.rgba, &reference, rigid);
    compare("floors' own pixels", &picture.rgba, &reference, |p| picture.ids[p] != 0 && class_at(picture.ids[p]) == Class::Floor);
    // the Sims' own pixels: where the last draw is a skinned part or the portrait's; and the
    // portrait's whole square, backdrop included
    compare("Sims' own pixels (skinned parts and the portrait's draws)", &picture.rgba, &reference, |p| picture.ids[p] != 0 && matches!(class_at(picture.ids[p]), Class::Skinned | Class::Overlay));
    if std::env::var_os("HF_SIMS").is_some() {
        let mut by: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for (p, &id) in picture.ids.iter().enumerate().filter(|x| *x.1 != 0 && matches!(class_at(*x.1), Class::Skinned | Class::Overlay)) {
            let off = (0..3).any(|c| (i32::from(picture.rgba[p * 4 + c]) - i32::from(reference[p * 4 + c])).abs() > 4);
            let e = by.entry(format!("{} strip {}", scene.drawn[id as usize - 1].model, scene.drawn[id as usize - 1].strip)).or_default();
            e.0 += 1;
            e.1 += u64::from(off);
        }
        for (k, v) in by {
            println!("  SIM {k}: {} px, {} off", v.0, v.1);
        }
    }
    // the portrait's viewport as the log has it, in whole pixels
    if let Some(v) = scene.frame.draws.iter().find_map(|d| d.viewport) {
        let (px, py, pw, ph) = v.pixels();
        compare("portrait square (the logged viewport)", &picture.rgba, &reference, |p| {
            let (x, y) = ((p % WIDTH as usize) as i32, (p / WIDTH as usize) as i32);
            (px..px + pw as i32).contains(&x) && (py..py + ph as i32).contains(&y)
        });
    }
    compare("shadows' own pixels", &picture.rgba, &reference, |p| picture.late_ids[p] != 0 && class_at(picture.late_ids[p]) == Class::Shadow);
    // where the error is: the models whose pixels are furthest off
    let mut by_model: BTreeMap<String, (u64, u64, [i64; 3])> = BTreeMap::new();
    for (p, &id) in picture.ids.iter().enumerate().filter(|x| rigid(x.0)) {
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
    if let Some(n) = std::env::var("HF_FLOORS").ok().and_then(|v| v.parse::<usize>().ok()) {
        // the floors' and shadows' pixels furthest off, by draw
        let mut by_draw: BTreeMap<(u32, bool), (u64, u64)> = BTreeMap::new();
        for p in 0..(WIDTH * HEIGHT) as usize {
            for (id, late) in [(picture.ids[p], false), (picture.late_ids[p], true)] {
                if id != 0 && matches!(class_at(id), Class::Floor | Class::Shadow) && (late == (class_at(id) == Class::Shadow)) {
                    let off = (0..3).any(|c| (i32::from(picture.rgba[p * 4 + c]) - i32::from(reference[p * 4 + c])).abs() > 4);
                    let e = by_draw.entry((id, late)).or_default();
                    e.0 += 1;
                    e.1 += u64::from(off);
                }
            }
        }
        let mut v: Vec<_> = by_draw.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse((x.1).1));
        for ((id, _), (cnt, off)) in v.into_iter().take(n) {
            let info = &scene.drawn[id as usize - 1];
            println!("  {off:>6} of {cnt:>6} off: {:?} {} {:?}", info.class, info.model, info.textures);
        }
    }
    if std::env::var_os("HF_FLAGS").is_some() {
        let mut by_flags: BTreeMap<u32, (usize, Vec<String>)> = BTreeMap::new();
        for d in &scene.drawn {
            let e = by_flags.entry(d.flags[0]).or_default();
            e.0 += 1;
            let t = format!("{}:{}", d.model, d.textures[0].clone().unwrap_or_default());
            if e.1.len() < 8 && !e.1.contains(&t) {
                e.1.push(t);
            }
        }
        for (f, (n, ex)) in by_flags {
            println!("flags {f:#x}: {n} draws e.g. {ex:?}");
        }
    }
    if let Ok(name) = std::env::var("HF_MODEL") {
        let mut shown = 0;
        for (p, &id) in picture.ids.iter().enumerate().filter(|x| *x.1 != 0) {
            let off = (0..3).any(|c| (i32::from(picture.rgba[p * 4 + c]) - i32::from(reference[p * 4 + c])).abs() > 4);
            if off && scene.drawn[id as usize - 1].model == name && p % 37 == 0 && shown < 25 {
                shown += 1;
                println!("  {},{}: ours {:?} reference {:?} (strip {})", p % WIDTH as usize, p / WIDTH as usize, &picture.rgba[p * 4..p * 4 + 3], &reference[p * 4..p * 4 + 3], scene.drawn[id as usize - 1].strip);
            }
        }
    }
    for probe in std::env::var("HF_PROBE").unwrap_or_default().split(';').filter(|p| !p.is_empty()) {
        let xy: Vec<usize> = probe.split(',').filter_map(|v| v.parse().ok()).collect();
        let p = xy[1] * WIDTH as usize + xy[0];
        let id = picture.ids[p];
        println!("probe {probe}: ours {:?} reference {:?}", &picture.rgba[p * 4..p * 4 + 4], &reference[p * 4..p * 4 + 4]);
        if id != 0 {
            let (info, d) = (&scene.drawn[id as usize - 1], scene.frame.draws.iter().find(|d| d.id == id).unwrap());
            println!("   {info:?}\n   {:?}\n   {:?}", d.material, d.lighting);
        }
        if picture.late_ids[p] != 0 {
            let (info, d) = (&scene.drawn[picture.late_ids[p] as usize - 1], scene.frame.draws.iter().find(|d| d.id == picture.late_ids[p]).unwrap());
            println!("   late: {info:?}\n   {:?}", d.material);
        }
    }
    // (HF_EXCLUDE="x0,y0,x1,y1;...": boxes another pass paints over, left out of a second figure)
    let boxes: Vec<Vec<usize>> = std::env::var("HF_EXCLUDE").unwrap_or_default().split(';').filter(|b| !b.is_empty()).map(|b| b.split(',').filter_map(|v| v.parse().ok()).collect()).collect();
    if !boxes.is_empty() {
        let inside = |p: usize| boxes.iter().any(|b| (b[0]..b[2]).contains(&(p % WIDTH as usize)) && (b[1]..b[3]).contains(&(p / WIDTH as usize)));
        compare("rigid draws' pixels outside the excluded boxes", &picture.rgba, &reference, |p| rigid(p) && !inside(p));
    }
    compare("whole frame", &picture.rgba, &reference, |_| true);

    let out = frames_dir.join(std::env::var("HF_OUT").unwrap_or_else(|_| "r1".into()));
    std::fs::create_dir_all(&out).unwrap();
    write_png(&out.join(format!("ours_{frame}.png")), &picture.rgba.as_chunks::<4>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect::<Vec<_>>());
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
    // where the shadows' passes drew (red: shadow draws, blue: the mask)
    let passes: Vec<u8> = (0..(WIDTH * HEIGHT) as usize)
        .flat_map(|p| {
            let g = picture.rgba[p * 4] / 2;
            match picture.late_ids[p] {
                0 => [g, g, g, 255],
                id if class_at(id) == Class::Shadow => [255, g, g, 255],
                _ => [g, g, 255, 255],
            }
        })
        .collect();
    write_png(&out.join(format!("passes_{frame}.png")), &passes);
    ExitCode::SUCCESS
}
