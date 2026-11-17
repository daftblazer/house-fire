//! `render-check LOG ISO FRAME`: renders one frame of the home lot from the observer log's
//! records with the engine's own renderer, and compares it with the recompiled game's own dump
//! of that frame (`$HF_FRAMES/world_NNNNN.png`, the picture before the 2D layer). Writes our
//! picture and a difference picture under `$HF_FRAMES/r1/` (`HF_OUT=name`: another folder).
//!
//! The engine mode (`HF_ENGINE=1`, with `--card CARD` for a save's lot; `--lot N` for lot N of
//! the disc's own save, the club's 15, with `--card` then naming only who the Sims are): the lot's model draws
//! (objects, walls, the lot's own model), its floors (by room, lit from the light grid), the
//! shadow pass and the camera (`scene::Camera` from the logged parameters) are made from the
//! engine's own world (`engine_scene`) in place of the log's records, in the original's order
//! (floors, the lot's ground, shadows, masks of each room's batch; then the models); the Sims
//! (their parts, shadows and the portrait) are made from the engine's own run of the log up to the
//! frame (`sims`), unless `HF_NO_SIMS=1` keeps the log's, and `HF_LOG_SKINS=1` their run-time
//! textures from the log's. Studies for it:
//! `HF_ORACLE=1` takes the light sets from the log's draw at the same world matrix,
//! `HF_MATDIFF=1` compares the materials and states with the log's, `HF_LIGHTSTUDY=1` prints
//! both light sets in world space, `HF_FLOORCMP=1` the engine's floor strips and their colours
//! against the log's, `HF_SHADOWDUMP=1` (or `mat`, `view`) and `HF_FLOORDUMP=1` (or `mat`) the
//! log's shadow and floor draws, `HF_LOGCAM=1` the logged view in place of the camera's,
//! `HF_SPHERE=1` the sphere-mapped draws, `HF_SHEARS=1` the shadows' shears, `HF_PLACES=1` the
//! objects' screen places, `HF_COLOURS=1` (with `HF_LIGHTSTUDY`) the lights' colour ratios,
//! `HF_FLOORS=n` the n floor and shadow draws furthest off,
//! `HF_DRAWS=prefix` the planned draws of a model, `HF_OBJECTS=1` the lot's objects,
//! `HF_LISTMODEL=name` a model's draws in draw order, `HF_ORDER=1` the whole draw order (runs
//! collapsed), `HF_SKIP=prefix` and `HF_SKIPTEX=name` leave a model's or a texture's engine draws
//! out, `HF_HOUSE=n` the lot's house number (default 1).
//!
//! Study aids (environment): `HF_WORST=n` lists the n models whose pixels are furthest off,
//! `HF_PROBE="x,y;x,y"` explains the draw at those pixels, `HF_MODEL=name` samples a model's
//! off pixels, `HF_FLAGS=1` counts the materials' flag words, `HF_EXCLUDE` leaves
//! boxes out of a second figure, `HF_VALIDATE=1` turns the Vulkan validation layer on.

#[path = "render-check/studies.rs"]
mod studies;

use housefire::disc::Disc;
use housefire::tree_log::read_log;
use housefire_render::gpu::Gpu;
use housefire_render::renderer::Renderer;
use housefire_render::engine_scene;
use housefire_render::sims;
use housefire_render::frame::{self, Lighting};
use housefire_render::scene::{Assets, Class, Scene};
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

/// The engine mode (`HF_ENGINE=1`): the lot's draws from the engine's own world in place of the log's
/// rigid and display-list draws, floors and shadows, and the Sims from its own run; the log's scene
/// is changed in place (`engine_scene::assemble`).
fn engine_frame(scene: &mut Scene, assets: &Assets, disc: &Disc, log: &housefire::tree_log::Log, tick: u32, card: Option<&Path>, lot_index: Option<usize>) {
    // the lot's house number (its HouseData row, the lot's global 10, which the loaded lot does not
    // keep): the disc's houses are numbered from 1 in the order of its lots (the club, lot 15, is
    // house 16); a card's lot is the home (house 1); HF_HOUSE says another
    // `--lot N` names a lot of the disc's own save (the club); the card then only says who the
    // Sims are (their descriptions are the played save's made Sims). Without it the card's lot is
    // the one the log ran on.
    let (cast_card, card) = (card, if lot_index.is_some() { None } else { card });
    let house = std::env::var("HF_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| if card.is_none() { lot_index.map_or(1, |k| k + 1) } else { 1 });
    // the camera: the engine's `scene::Camera` from the logged parameters (the lot's start has
    // no player input to run it from)
    let mut view = scene.frame.camera.view;
    if let Some((v, pr)) = engine_scene::camera(log, tick).filter(|_| std::env::var_os("HF_LOGCAM").is_none()) {
        let dv = v.iter().zip(view.iter()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        let dp = pr.iter().zip(scene.frame.camera.projection.iter()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        println!("camera from the logged parameters against the logged view: view {dv:.2e}, projection {dp:.2e}");
        view = v;
        scene.frame.camera = frame::Camera { view: v, projection: pr };
    }
    // the run to the tick before this one: the Sims' world, and what each object plays (its
    // graphic and the render frames since a script set it); HF_NO_SIMS=1 keeps the log's Sims
    // and the loaded lot's plays
    let lib = housefire::library::Library::bustin_out(disc);
    let start = housefire::replay::Start::read(disc, card.and_then(|p| p.to_str()), lot_index.map_or(-1, |k| k as i64)).expect("the lot");
    let catalog = housefire::anim::Catalog::read(disc, house as u32);
    let mut replay = housefire::replay::Replay::new(log, &lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, housefire::replay::RunWith { card_lot: start.card_lot, cat: Some(catalog.clone()) }).expect("the run");
    let no_sims = std::env::var_os("HF_NO_SIMS").is_some();
    let plays = if no_sims { housefire::drawlist::Plays::at(tick) } else { engine_scene::run_to(&mut replay, log, tick) };
    let plan = engine_scene::plan(disc, card, log, tick, &view, (house, lot_index), &plays).expect("the engine's lot");
    let shaders = housefire::shader::index(disc);
    // HF_SHADER=prefix: the shaders whose name starts so (layers' flags, words, bytes, values)
    if let Ok(prefix) = std::env::var("HF_SHADER") {
        let mut seen = std::collections::BTreeSet::new();
        for s in shaders.values().filter(|s| s.name.to_lowercase().starts_with(&prefix.to_lowercase())) {
            if seen.insert(s.name.clone()) {
                println!("  shader {} head {:02x?}", s.name, &s.head[..]);
                for l in &s.layers {
                    println!("    layer tex {:08x} flags {:#x} word {:#x} bytes {:02x?} value {}", l.texture, l.flags, l.word, l.bytes, l.value);
                }
                println!("    tail {:02x?}", s.tail);
            }
        }
    }
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
    let lighting = |d: &housefire::drawlist::Draw, mat: engine_scene::MaterialFactors| {
        if std::env::var_os("HF_ORACLE").is_none() {
            return engine_scene::lighting_for(&plan.lights.of(d), &view, mat);
        }
        let near = |a: &[f32; 12]| (0..12).all(|i| (a[i] - d.world[i]).abs() <= 1e-4 * if i % 4 == 3 { 1.0 } else { a[0].abs().max(a[5].abs()).max(a[10].abs()).max(1e-9) });
        match oracle.iter().find(|o| near(&o.0)) {
            Some(o) => o.1.clone(),
            None => Lighting::Lit { ambient: [0.5; 3], directionals: Vec::new(), points: Vec::new() },
        }
    };
    let mut caches = engine_scene::Caches::default();
    let made = assets.engine_draws(scene, &mut caches, &engine_scene::Inputs { draws: &plan.draws, models: &plan.models, shaders: &shaders, lighting: &lighting, view });
    if std::env::var_os("HF_SPHERE").is_some() {
        studies::sphere_engine(&made);
    }
    // (HF_SKIP=prefix and HF_SKIPTEX=name, study aids: leave out the engine's draws of the
    // models with that name, or of the materials whose first layer has that texture)
    let mut made = made;
    if let Ok(prefix) = std::env::var("HF_SKIP") {
        made.retain(|m| !m.info.model.starts_with(&prefix));
    }
    if let Ok(texture) = std::env::var("HF_SKIPTEX") {
        made.retain(|m| m.info.textures[0].as_deref() != Some(texture.as_str()));
    }
    let planned = made.len();
    if std::env::var_os("HF_LIGHTSTUDY").is_some() {
        studies::light_study(&made, &oracle, &plan, &shaders, scene, view);
    }
    if std::env::var_os("HF_MATDIFF").is_some() {
        studies::material_diff(&made, &oracle, scene, view);
    }
    // the floors by room, the shadow pass (the pieces' shadows lie on the outdoors' floors, an
    // object's on its room's) and the lot's own ground, then the order of the original
    if std::env::var_os("HF_FLOORCMP").is_some() {
        studies::floor_cmp(&plan, log, tick);
    }
    let floors: Vec<(usize, engine_scene::FloorDraws)> = plan.floors.iter().zip(assets.engine_floors(scene, &mut caches, &plan.floors, &shaders, plan.terrain, view)).map(|(st, f)| (st.room, f)).collect();
    let sun = plan.lights.at(1.0, 1.0).sun.map_or([0.0; 3], |s| s.direction);
    let projection = scene.frame.camera.projection;
    if std::env::var_os("HF_PLACES").is_some() {
        studies::places(&plan, scene, view);
    }
    if std::env::var_os("HF_SHEARS").is_some() {
        studies::shears_engine(&plan, scene);
    }
    let by_room = assets.engine_shadows_by_room(scene, &mut caches, &plan, &view, &projection, sun);
    // the Sims from the engine's own run (HF_NO_SIMS=1 keeps the log's): the replay to the tick
    // before this one for the world, the animation frames through this one for the poses
    let sims = if no_sims {
        None
    } else {
        let ngh = engine_scene::save_file(disc, cast_card).expect("the save");
        let rigs = housefire::figure::Rigs::read(disc).expect("the skeletons");
        let mut cast = housefire::figure::Cast::read(disc, &ngh).expect("the part tables");
        // the Sims the save does not describe wear what the log's dressing records say
        let who: Vec<i16> = replay.world().objects().filter(|o| o.person && o.on_lot()).filter_map(|o| o.person_data.get(31).copied()).collect();
        let logged: Vec<housefire::outfit::Description> = log.dressed.iter().map(|r| housefire::outfit::Description::from_words(&r.description_words)).collect();
        let given = cast.complete_from(&who, &logged);
        println!("{given} of {} Sims described from the log's dressing records", who.len());
        let skins = sims::SimSkins::from_scene(scene).with_disc(disc);
        let wardrobe = sims::Wardrobe { rigs: &rigs, cast: &cast, catalog: &catalog, skins: &skins, models: &plan.models, shaders: &shaders };
        let plays = replay.sim_plays(tick);
        let mut figures = sims::figures_of(replay.world(), &cast, &plays, &replay.sim_faces());
        if let Some(skip) = std::env::var("HF_SKIPSIM").ok().and_then(|v| v.parse::<i16>().ok()) {
            figures.retain(|f| f.id != skip);
        }
        let portrait = sims::portrait_sim(scene, &wardrobe, &figures);
        for f in &figures {
            println!("Sim {} at {:?} heading {} (facing {}) play {:?}{}", f.id, f.place, f.heading, replay.world().find_object(f.id).map_or(0, |o| o.facing()), f.play, if portrait == Some(f.id) { ", the portrait's" } else { "" });
        }
        // HF_SIMSTATE=1: each Sim's data fields, person data and sprite flags that are not zero (study aid)
        if std::env::var_os("HF_SIMSTATE").is_some() {
            for f in &figures {
                if let Some(o) = replay.world().find_object(f.id) {
                    let nz = |v: &[i16]| v.iter().enumerate().filter(|x| *x.1 != 0).map(|(i, x)| format!("{i}={x}")).collect::<Vec<_>>().join(" ");
                    println!("simstate {} level {} x16 {} y16 {} data[{}] pdata[{}] sprite[{}] queue {}", f.id, o.level, o.x16, o.y16, nz(&o.data), nz(&o.person_data), nz(&o.sprite_flags), o.queue.len());
                }
            }
        }
        if std::env::var_os("HF_HEADINGS").is_some() {
            for m in sims::bones_matched(log, tick, &wardrobe, &figures) {
                println!("bones matched: Sim {} heading {} {} of {}", m.0, m.1, m.2, m.3);
            }
            let hs = sims::headings_from_log(log, tick, &wardrobe, &figures);
            let mut seen: Vec<[f32; 12]> = Vec::new();
            for k in log.skins.iter().filter(|k| k.tick == tick) {
                for m in k.bones {
                    if !seen.contains(&m) {
                        seen.push(m);
                    }
                }
            }
            for m in &seen {
                println!("logged bone {:?}", [m[3], m[7], m[11]]);
            }
            for f in &figures {
                println!("heading study: Sim {} ours {} the log's {:?} at {:?}", f.id, f.heading, hs.get(&f.id), f.place);
            }
        }
        let mut sim_cache = sims::SimCaches::default();
        Some(assets.engine_sims(scene, &mut caches, &mut sim_cache, &wardrobe, &figures, &plan.lights, sun, view, portrait))
    };
    let (lot, objects): (Vec<engine_scene::Made>, Vec<engine_scene::Made>) = made.into_iter().partition(|m| plan.draws[m.from].kind == housefire::drawlist::Kind::LotList && m.draw.material.blend == frame::Blend::Opaque);
    println!("engine floors: {} strips in rooms {:?}; shadows {} planned, per room {:?}", floors.len(), floors.iter().map(|f| f.0).collect::<std::collections::BTreeSet<_>>(), plan.shadows.len(), by_room.iter().map(Vec::len).collect::<Vec<_>>());
    let removed = engine_scene::assemble(scene, engine_scene::Engine { floors, lot, shadows: by_room, objects, sims });
    println!("engine mode: {} draws planned, {planned} made; the log's {removed} floor, shadow, rigid and display-list draws left out", plan.draws.len());
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
    // A frame's strips: from the frame record of its copies before it (frame - 1) to the next
    // one, by the strips before them. Two frames can share a tick; then the tick's strips outside
    // the frame's are left out below.
    let strip_range = log.frame_strips.iter().position(|f| f.0 == frame.wrapping_sub(1)).map(|i| {
        let start = log.frame_strips[i].1;
        let end = log.frame_strips[i + 1..].iter().find(|f| f.0 >= frame && f.1 > start).map_or(log.strips.len(), |f| f.1);
        start..end
    });
    let from_records = strip_range.as_ref().and_then(|r| log.strips.get(r.start)).map(|s| s.tick);
    let Some(tick) = from_records.or_else(|| ticks.iter().nth(frame.wrapping_sub(first_frame) as usize).copied()) else {
        eprintln!("the log has draws for frames {first_frame}.. ({} of them)", ticks.len());
        return ExitCode::FAILURE;
    };
    let shared_tick = strip_range.clone().filter(|r| log.strips.iter().enumerate().any(|(i, s)| s.tick == tick && !r.contains(&i)));
    let reference = read_png(&frames_dir.join(format!("world_{frame:05}.png"))).expect("the reference picture");

    let assets = Assets::new(&disc);
    let mut scene = assets.scene(&log, tick);
    // the engine mode (HF_ENGINE=1, with --card CARD for a save's lot): the lot's model draws from
    // the engine's own state in place of the log's rigid and display-list draws
    if std::env::var_os("HF_SPHERE").is_some() {
        studies::sphere_log(&scene);
    }
    if std::env::var_os("HF_SHEARS").is_some() {
        studies::shears_log(&scene);
    }
    let engine_mode = std::env::var_os("HF_ENGINE").is_some();
    if engine_mode {
        let card = args.iter().position(|a| a == "--card").and_then(|i| args.get(i + 1)).map(PathBuf::from);
        let lot_index = args.iter().position(|a| a == "--lot").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok());
        engine_frame(&mut scene, &assets, &disc, &log, tick, card.as_deref(), lot_index);
    }

    if std::env::var_os("HF_SKINDRAWS").is_some() {
        for d in &scene.frame.draws {
            let info = &scene.drawn[d.id as usize - 1];
            if let Some(k) = info.skin {
                let b = log.skins[k].bones[0];
                println!("skin draw {k}: bone0 at ({:.2}, {:.2}, {:.2}) {} blend {:?} depth {:?} colour {} cull {:?}", b[3], b[7], b[11], info.model, d.material.blend, d.material.depth_test, d.material.write_colour, d.material.cull);
            }
        }
    }
    if std::env::var_os("HF_FLOORDUMP").is_some() {
        studies::floor_dump(&scene);
    }
    if std::env::var_os("HF_SHADOWDUMP").is_some() {
        studies::shadow_dump(&scene);
    }
    if std::env::var_os("HF_ORDER").is_some() {
        studies::draw_order(&scene);
    }
    if let Ok(name) = std::env::var("HF_LISTMODEL") {
        studies::list_model(&scene, &name);
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
    // HF_DROPSTRIP="lo,hi" (study aid, log mode): leave out the model draws of the log's strips lo..hi
    if let Some((lo, hi)) = std::env::var("HF_DROPSTRIP").ok().and_then(|v| v.split_once(',').map(|(a, b)| (a.parse::<usize>().unwrap_or(0), b.parse::<usize>().unwrap_or(0)))) {
        let drawn = &scene.drawn;
        scene.frame.draws.retain(|d| {
            let i = &drawn[d.id as usize - 1];
            !(matches!(i.class, Class::RigidStrip | Class::DisplayList) && (lo..hi).contains(&i.strip))
        });
    }
    // a tick holding two frames: only this frame's strips (log mode's model draws)
    if let Some(r) = shared_tick.as_ref().filter(|_| !engine_mode) {
        let drawn = &scene.drawn;
        scene.frame.draws.retain(|d| {
            let i = &drawn[d.id as usize - 1];
            !matches!(i.class, Class::RigidStrip | Class::DisplayList) || r.contains(&i.strip)
        });
        println!("frame {frame}: strips {}..{} of tick {tick}, which holds another frame's too", r.start, r.end);
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

    let class_at = |id: u32| scene.drawn[id as usize - 1].class;
    let rigid = |p: usize| picture.ids[p] != 0 && matches!(class_at(picture.ids[p]), Class::RigidStrip | Class::DisplayList);
    compare("rigid draws' pixels", &picture.rgba, &reference, rigid);
    compare("floors' own pixels", &picture.rgba, &reference, |p| picture.ids[p] != 0 && class_at(picture.ids[p]) == Class::Floor);
    // the Sims' own pixels: where the last draw is a skinned part or the portrait's; and the
    // portrait's whole square, backdrop included
    compare("Sims' own pixels (skinned parts and the portrait's draws)", &picture.rgba, &reference, |p| picture.ids[p] != 0 && matches!(class_at(picture.ids[p]), Class::Skinned | Class::Overlay));
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
            if off && scene.drawn[id as usize - 1].model[..] == name && p % 37 == 0 && shown < 25 {
                shown += 1;
                println!("  {},{}: ours {:?} reference {:?} (strip {})", p % WIDTH as usize, p / WIDTH as usize, &picture.rgba[p * 4..p * 4 + 3], &reference[p * 4..p * 4 + 3], scene.drawn[id as usize - 1].strip);
            }
        }
    }
    // HF_STATES=1: the model draws counted by (model, texture, blend, alpha test, depth write),
    // to set against the other mode's
    if std::env::var_os("HF_STATES").is_some() {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for d in scene.frame.draws.iter().filter(|d| d.projection.is_none()) {
            let info = &scene.drawn[d.id as usize - 1];
            if matches!(info.class, Class::RigidStrip | Class::DisplayList) {
                let m = &d.material;
                *counts.entry(format!("{} {:?} {:?} {:?} dw{} {} {:?}", info.model, info.textures[0], m.blend, m.alpha_test.map(|a| a.refs[0]), m.depth_write, match d.lighting { Lighting::Lit { .. } => "lit", Lighting::VertexColours => "vcol", Lighting::Black => "black" }, m.combine)).or_default() += 1;
            }
        }
        for (k, n) in counts {
            println!("states {n} {k}");
        }
    }
    // HF_MATS=name: that model's draws in draw order, with their view depth and world matrices
    if let Ok(only) = std::env::var("HF_MATS") {
        let v = scene.frame.camera.view;
        for d in scene.frame.draws.iter().filter(|d| d.projection.is_none()) {
            let info = &scene.drawn[d.id as usize - 1];
            if !matches!(info.class, Class::RigidStrip | Class::DisplayList) || info.model[..] != only {
                continue;
            }
            let (x, y, z) = (d.world[3], d.world[7], d.world[11]);
            let depth = v[8] * x + v[9] * y + v[10] * z + v[11];
            let dist = ((v[0] * x + v[1] * y + v[2] * z + v[3]).powi(2) + (v[4] * x + v[5] * y + v[6] * z + v[7]).powi(2) + depth * depth).sqrt();
            println!("mats {:5} depth {depth:8.3} dist {dist:8.3} {} s{} {:?} {}", d.id, info.model, info.strip, info.textures[0], d.world.map(|v| format!("{v:.3e}")).join(" "));
        }
    }
    // HF_MODELMAP=name: the pixels of that model's draws, off ones in red, the rest in grey
    if let Ok(name) = std::env::var("HF_MODELMAP") {
        let map: Vec<u8> = (0..(WIDTH * HEIGHT) as usize)
            .flat_map(|p| {
                let id = picture.ids[p] as usize;
                let g = reference[p * 4] / 3;
                if id == 0 || scene.drawn[id - 1].model[..] != name {
                    return [g, g, g, 255];
                }
                let off = (0..3).any(|c| (i32::from(picture.rgba[p * 4 + c]) - i32::from(reference[p * 4 + c])).abs() > 4);
                if off { [255, 0, 0, 255] } else { [0, 200, 0, 255] }
            })
            .collect();
        let dir = frames_dir.join(std::env::var("HF_OUT").unwrap_or_else(|_| "r1".into()));
        std::fs::create_dir_all(&dir).unwrap();
        write_png(&dir.join("modelmap.png"), &map);
        // the same model's pixels by the texture of the last draw there
        let mut by_texture: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for p in 0..(WIDTH * HEIGHT) as usize {
            let id = picture.ids[p] as usize;
            if id != 0 && scene.drawn[id - 1].model[..] == name {
                let off = (0..3).any(|c| (i32::from(picture.rgba[p * 4 + c]) - i32::from(reference[p * 4 + c])).abs() > 4);
                let e = by_texture.entry(format!("{:?}", scene.drawn[id - 1].textures[0])).or_default();
                e.0 += 1;
                e.1 += usize::from(off);
            }
        }
        for (t, (n, off)) in by_texture {
            println!("  {name} {t}: {off} of {n} off");
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
