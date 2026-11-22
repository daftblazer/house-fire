//! `ui-check LOG ISO FRAME...`: plays the Apt player over the observer log to each frame, draws
//! its display list on the GPU over the game's picture of that frame before its 2D layer
//! (`$HF_FRAMES/ui/world_NNNNN.png`), and compares with the game's own picture
//! (`frame_NNNNN.png`) on the 2D layer: the pixels where the game's two pictures differ by more
//! than 4 in a channel. The portrait square (the game's own 3D draw, not the movies') is left
//! out. Also counts `over`: the pixels we draw where the game's frame is its world picture. The
//! game's own text comes from the log's text records (`game_text`). Writes ours and a difference
//! picture under `$HF_UI_OUT` (default `$HF_FRAMES/ui-out/`; the frames' folder under
//! `$HF_FRAMES` is `$HF_UI_DIR`, default `ui`).
//!
//! `--native [--card CARD] [--lot N]`: the movies are run by the engine's own feed
//! (`housefire::apt::ui::Ui` over a replay of the log's world) instead of the log's movie
//! records. The log gives only what a player would press: the lot's start (`loadFunctions` and
//! `showHUD` of the game's first frames), the pause screen's opening and closing, and the frames'
//! ticks; the dialog opens and closes with the world's own Dialog node.

use housefire::apt::display::DisplayItem;
use housefire::apt::play;
use housefire::apt::ui::{Feed, Ui};
use housefire::hud::{Dialog, Hud, Measure, PlayerView, Speed};
use housefire::library::Library;
use housefire::replay::{Replay, RunWith, Start};
use housefire::shader::name_hash;
use housefire::disc::Disc;
use housefire::tree_log::{Log, read_log};
use housefire_render::apt_draw::{AptAssets, Painter, TextRun};
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


/// What the engine's own feed draws at some game frames: the display list and the lines.
type Pictures = std::collections::BTreeMap<u32, (Vec<DisplayItem>, Vec<TextRun>)>;

/// Runs the interface from the engine's feed over a replay of the log's world and keeps what
/// it shows at each of `wanted` (a picture of frame N shows the interface's frame N - `lag`).
fn native_pictures(log: &Log, disc: &Disc, assets: &AptAssets, card: Option<&str>, lot: i64, wanted: &[u32], lag: u32) -> Option<Pictures> {
    let start = Start::read(disc, card, lot).ok()?;
    let ngh = card.and_then(|c| std::fs::read(c).ok()).and_then(|mut b| {
        b.truncate(1 << 22);
        housefire::save::memory_card_file(&b, "G4ME69").ok()
    });
    let names = housefire::figure::sim_names(&ngh.unwrap_or_else(|| disc.read("Runtime/default.ngh").unwrap_or_default()));
    let lib = Library::bustin_out(disc);
    let house = std::env::var("REPLAY_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let catalog = housefire::anim::Catalog::read(disc, house);
    let mut replay = Replay::new(log, &lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, goals: start.goals.clone(), neighbourhood: start.neighbourhood.clone(), cat: Some(catalog) })?;
    let font = assets.fonts.iter().find(|(n, _)| name_hash(n) == 0x67f6_64fb)?.1;
    let measure = Measure { font };
    // the log's frames: the last tick each had (the lot is played from tick 1), and the markers
    // of what the player did
    let mut tick_of: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut note = |frame: u32, tick: u32| {
        let t = tick_of.entry(frame).or_default();
        *t = (*t).max(tick);
    };
    for c in log.movie_calls.iter().filter(|c| c.tick > 0) {
        note(c.frame, c.tick);
    }
    for r in log.movie_reads.iter().filter(|r| r.tick > 0) {
        note(r.frame, r.tick);
    }
    for t in log.text_draws.iter().filter(|t| t.tick > 0) {
        note(t.frame, t.tick);
    }
    let first_call = |name: &str, from: u32| log.movie_calls.iter().find(|c| c.name == name && c.frame >= from).map(|c| c.frame);
    let f0 = log.movie_calls.iter().find(|c| c.name == "loadFunctions" && c.args.first().is_some_and(|a| a.1 == "hud")).map(|c| c.frame)?;
    let show_at = first_call("showHUD", f0)?;
    let opens: Vec<u32> = log.movie_calls.iter().filter(|c| c.name == "showPause").map(|c| c.frame).collect();
    let closes: Vec<u32> = log.movie_calls.iter().filter(|c| c.name == "hidePause").map(|c| c.frame).collect();
    let speed_at = |tick: u32| {
        let s = log.tracks.iter().filter(|t| t.tick == tick).map(|t| t.speeds[1].abs()).fold(0.0, f32::max);
        if s == 10.0 {
            Speed::Fastest
        } else if s == 5.0 {
            Speed::Fast
        } else {
            Speed::Normal
        }
    };
    let last = wanted.iter().max().copied()?.saturating_sub(lag);
    let mut ui = Ui::new(assets.flashes.clone());
    ui.set_game(housefire::apt::pause::Game::new(&disc.read("quickdat.arc").unwrap_or_default(), &lib, start.family_funds()));
    ui.set_frame(f0);
    if std::env::var_os("HF_UI_LOGREADS").is_some() {
        ui.player_mut().set_reads(log.movie_reads.iter().map(|r| (r.frame, r.name.clone(), r.value.clone())));
    }
    let mut hud = Hud::new(1);
    let (mut dialog_open, mut out) = (false, Pictures::new());
    let mut tick = 0;
    for n in f0..=last {
        if let Some((_, &t)) = tick_of.range(..=n).next_back() {
            tick = t;
        }
        while replay.tick() < tick && replay.step() {}
        let world = replay.world();
        let sim = world.objects().filter(|o| o.person && o.person_data.get(32) == Some(&0)).map(|o| o.id()).min()?;
        let view = PlayerView::of(world, sim)?;
        if n == show_at {
            ui.begin_play(&mut hud, &view);
        }
        if !dialog_open && world.dialog_object != 0 {
            let (object, operand) = (world.dialog_object, world.dialog_operand);
            let strings: Vec<String> = world.find_object(object).and_then(|o| o.file).and_then(|f| lib.file(f).strings.get(&301)).cloned().unwrap_or_default();
            let name = |id: i16| match world.find_object(id) {
                Some(o) if o.person => o.person_data.get(31).and_then(|k| names.get(k)).cloned().unwrap_or_else(|| format!("object {id}")),
                Some(o) => o.file.and_then(|f| lib.file(f).catalog.values().next().and_then(|s| s.first()).cloned()).unwrap_or_else(|| format!("object {id}")),
                None => format!("object {id}"),
            };
            hud.show_dialog(Dialog::opened(&strings, operand, &world.dialog_locals, &name, &measure));
            dialog_open = true;
        }
        if dialog_open && world.dialog_object == 0 {
            hud.hide_dialog();
            dialog_open = false;
        }
        if log.pads.is_empty() {
            if opens.contains(&n) {
                ui.open_pause(&mut hud);
            }
            if closes.contains(&n) {
                ui.close_pause(&mut hud, &view);
            }
        } else {
            // a log with the pad (0x81 records): the pause screen runs from it, as LOCKSTEP_PAUSE runs it
            let buttons = log.pads.iter().take_while(|p| p.frame <= n).last().map_or(0, |p| p.buttons);
            ui.pad(buttons, &mut hud);
        }
        let speed = if ui.paused() { Speed::Paused } else { speed_at(tick) };
        let texts = ui.step(&mut Feed { hud: &mut hud, view: &view, speed, measure: &measure });
        if wanted.iter().any(|&w| w.saturating_sub(lag) == n) {
            if std::env::var_os("HF_UI_TEXTS").is_some() {
                for t in &texts {
                    println!("native text {:?} at ({:.5}, {:.5}) shadow {} width {:?} height {:?}", t.text, t.x, t.y, t.shadow, t.width, t.height);
                }
                for t in log.text_draws.iter().filter(|t| t.frame == n) {
                    println!("log text {:?} at ({:.5}, {:.5}) size words {:?}", t.text, t.x, t.y, t.size_words.map(f32::from_bits));
                }
            }
            out.insert(n + lag, (ui.display_list(), game_text::native_lines(assets, &texts)));
        }
    }
    if std::env::var_os("HF_UI_COMMANDS").is_some() {
        for c in &ui.sent {
            println!("native command: frame {} {} {}", c.frame, c.name, c.argument);
        }
    }
    Some(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let frames: Vec<u32> = args.iter().skip(3).filter_map(|a| a.parse().ok()).collect();
    let native = args.iter().any(|a| a == "--native");
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    if args.len() < 4 || frames.is_empty() {
        eprintln!("usage: ui-check LOG ISO [--native [--card CARD] [--lot N]] FRAME...   (HF_FRAMES: the folder holding ui/frame_NNNNN.png and ui/world_NNNNN.png)");
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
    let pictures = if native {
        match native_pictures(&log, &disc, &assets, opt("--card").as_deref(), opt("--lot").and_then(|v| v.parse().ok()).unwrap_or(-1), &frames, lag) {
            Some(p) => Some(p),
            None => {
                eprintln!("ui: the native run could not start");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let (mut total, mut total_within, mut total_over, mut scored) = (0usize, 0usize, 0usize, 0usize);
    for &frame in &frames {
        let ui = frames_dir.join(std::env::var("HF_UI_DIR").unwrap_or_else(|_| "ui".into()));
        let (Some(world), Some(game)) = (read_png(&ui.join(format!("world_{frame:05}.png"))), read_png(&ui.join(format!("frame_{frame:05}.png")))) else {
            eprintln!("ui: frame {frame}: no pictures in {}", ui.display());
            return ExitCode::FAILURE;
        };
        // a world picture that is all black (frames 760 and 1390 of the dump) has no picture to
        // lay the layer over: the 2D layer is then the whole frame, not the movies' alone
        let black = world.chunks(4).all(|p| p[..3].iter().all(|&c| c < 8));
        let (list, runs) = match &pictures {
            Some(p) => p.get(&frame).cloned().unwrap_or_default(),
            None => {
                let (player, _) = play::run(&log, assets.flashes.clone(), Some(frame.saturating_sub(lag)), play::Timing::default());
                (player.display_list(), game_text::lines(&assets, &log.text_draws, frame.saturating_sub(lag)))
            }
        };
        let mut drawing = painter.draw(&mut pass, &list);
        for run in &runs {
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
