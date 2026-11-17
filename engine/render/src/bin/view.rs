//! The window: the engine's own run of the home lot, drawn each tick from its own state.
//!
//! Usage: view LOG ISO --card CARD [--lot N] [--ticks N] [--frame-times FILE] [--shot TICK FILE]
//!
//! The log is the replay's input (`housefire::replay::Replay`): the engine steps its world one
//! tick at a time, 30 a second, and each tick the lot is drawn from that world through the
//! camera the log's 'c' records give (`engine_scene::Builder`), then shown (a Vulkan swapchain,
//! FIFO). The meshes and textures go to the GPU once.
//!
//! --frame-times: one line a frame: the tick, when its frame began (ms since the first), the
//! time since the frame before, the simulation's, the frame's build, the frame's whole CPU time
//! (waiting for the GPU included) and the GPU's own, in ms. --ticks: stop after N ticks. --shot:
//! save the picture of a tick (640x448) as PNG. HF_VALIDATE=1: the validation layer.

use ash::vk;
use housefire::disc::Disc;
use housefire::library::Library;
use housefire::replay::{Replay, RunWith, Start, atoi};
use housefire::tree_log::read_log;
use housefire_render::engine_scene::{self, Builder, Lot};
use housefire_render::frame::{Frame, TRANSIENT_MESH};
use housefire_render::gpu::Gpu;
use housefire_render::renderer::{Present, Renderer};
use housefire_render::scene::Assets;
use housefire_render::sims::{self, SimSkins, Wardrobe};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

/// The picture's size: the game's own (the reference pictures').
const WIDTH: u32 = 640;
const HEIGHT: u32 = 448;
/// The simulation's ticks a second, a frame each.
const TICKS_PER_SECOND: f64 = 30.0;
/// The home lot's house number (`HouseData`'s row, as `render-check` takes it).
const HOME_HOUSE: usize = 1;
/// How long before a frame's time the wait for it turns from sleeping into spinning (the sleep's
/// own overshoot).
const SPIN: Duration = Duration::from_micros(1500);

struct Args {
    log: String,
    iso: String,
    card: Option<String>,
    lot: i64,
    ticks: Option<u32>,
    frame_times: Option<PathBuf>,
    shot: Option<(u32, PathBuf)>,
}

fn args() -> Option<Args> {
    let a: Vec<String> = std::env::args().collect();
    let mut out = Args { log: a.get(1)?.clone(), iso: a.get(2)?.clone(), card: None, lot: -1, ticks: None, frame_times: None, shot: None };
    let mut i = 3;
    while i < a.len() {
        match a[i].as_str() {
            "--card" => out.card = Some(a.get(i + 1)?.clone()),
            "--lot" => out.lot = atoi(a.get(i + 1)?),
            "--ticks" => out.ticks = Some(a.get(i + 1)?.parse().ok()?),
            "--frame-times" => out.frame_times = Some(a.get(i + 1)?.into()),
            "--shot" => {
                out.shot = Some((a.get(i + 1)?.parse().ok()?, a.get(i + 2)?.into()));
                i += 1;
            }
            _ => return None,
        }
        i += 2;
    }
    Some(out)
}

/// The window, as the event loop makes it.
#[derive(Default)]
struct App {
    window: Option<Window>,
    closed: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = Window::default_attributes().with_title("House Fire").with_inner_size(PhysicalSize::new(WIDTH, HEIGHT)).with_resizable(false);
            self.window = Some(event_loop.create_window(attributes).expect("a window"));
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.closed = true;
        }
    }
}

/// The instance extensions the window system's surfaces need.
fn surface_extensions(display: RawDisplayHandle) -> Result<Vec<*const std::ffi::c_char>, String> {
    let platform = match display {
        RawDisplayHandle::Wayland(_) => ash::khr::wayland_surface::NAME,
        RawDisplayHandle::Xlib(_) => ash::khr::xlib_surface::NAME,
        RawDisplayHandle::Xcb(_) => ash::khr::xcb_surface::NAME,
        d => return Err(format!("no surface for {d:?}")),
    };
    Ok(vec![ash::khr::surface::NAME.as_ptr(), platform.as_ptr()])
}

/// The instance extensions gamescope's window-system layer wants besides: it forces
/// VK_EXT_swapchain_maintenance1 on the device, which needs these on the instance (else the
/// validation layer reports the device's creation). Only those the loader lists.
fn layer_extensions(entry: &ash::Entry) -> Vec<*const std::ffi::c_char> {
    let have = unsafe { entry.enumerate_instance_extension_properties(None) }.unwrap_or_default();
    let listed = |name: &std::ffi::CStr| have.iter().any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name));
    let wanted = [ash::ext::surface_maintenance1::NAME, ash::khr::get_surface_capabilities2::NAME];
    if wanted.iter().all(|n| listed(n)) { wanted.iter().map(|n| n.as_ptr()).collect() } else { Vec::new() }
}

fn make_surface(gpu: &Gpu, display: RawDisplayHandle, window: RawWindowHandle) -> Result<vk::SurfaceKHR, String> {
    let (entry, instance) = (&gpu.entry, &gpu.instance);
    let made = match (display, window) {
        (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(w)) => {
            let info = vk::WaylandSurfaceCreateInfoKHR::default().display(d.display.as_ptr()).surface(w.surface.as_ptr());
            unsafe { ash::khr::wayland_surface::Instance::new(entry, instance).create_wayland_surface(&info, None) }
        }
        (RawDisplayHandle::Xlib(d), RawWindowHandle::Xlib(w)) => {
            let info = vk::XlibSurfaceCreateInfoKHR::default().dpy(d.display.ok_or("no X display")?.as_ptr()).window(w.window);
            unsafe { ash::khr::xlib_surface::Instance::new(entry, instance).create_xlib_surface(&info, None) }
        }
        (RawDisplayHandle::Xcb(d), RawWindowHandle::Xcb(w)) => {
            let info = vk::XcbSurfaceCreateInfoKHR::default().connection(d.connection.ok_or("no X connection")?.as_ptr()).window(w.window.get());
            unsafe { ash::khr::xcb_surface::Instance::new(entry, instance).create_xcb_surface(&info, None) }
        }
        (d, w) => return Err(format!("no surface for {d:?} and {w:?}")),
    };
    made.map_err(|e| format!("create surface: {e}"))
}

/// The swapchain the frames are presented on, FIFO (every display has it, and it paces the
/// presents to the display's refresh), one release semaphore per image.
struct Swapchain {
    surface_loader: ash::khr::surface::Instance,
    loader: ash::khr::swapchain::Device,
    surface: vk::SurfaceKHR,
    chain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    extent: vk::Extent2D,
    acquire: vk::Semaphore,
    release: Vec<vk::Semaphore>,
}

impl Swapchain {
    fn new(gpu: &Gpu, surface: vk::SurfaceKHR, window: PhysicalSize<u32>) -> Result<Swapchain, String> {
        let surface_loader = ash::khr::surface::Instance::new(&gpu.entry, &gpu.instance);
        let supported = unsafe { surface_loader.get_physical_device_surface_support(gpu.phys, gpu.family, surface) }.map_err(|e| e.to_string())?;
        if !supported {
            return Err("the graphics queue cannot present to the window".into());
        }
        let caps = unsafe { surface_loader.get_physical_device_surface_capabilities(gpu.phys, surface) }.map_err(|e| e.to_string())?;
        if !caps.supported_usage_flags.contains(vk::ImageUsageFlags::TRANSFER_DST) {
            return Err("the swapchain's images cannot be blitted to".into());
        }
        let formats = unsafe { surface_loader.get_physical_device_surface_formats(gpu.phys, surface) }.map_err(|e| e.to_string())?;
        // the picture is plain bytes: a UNORM format takes them as they are (an sRGB one would encode them)
        let format = formats.iter().find(|f| matches!(f.format, vk::Format::B8G8R8A8_UNORM | vk::Format::R8G8B8A8_UNORM)).copied().ok_or("the surface has no 8-bit UNORM format")?;
        let extent = if caps.current_extent.width == u32::MAX { vk::Extent2D { width: window.width, height: window.height } } else { caps.current_extent };
        let wanted = caps.min_image_count + 1;
        let count = if caps.max_image_count == 0 { wanted } else { wanted.min(caps.max_image_count) };
        let loader = ash::khr::swapchain::Device::new(&gpu.instance, &gpu.device);
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .pre_transform(caps.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true);
        let chain = unsafe { loader.create_swapchain(&info, None) }.map_err(|e| format!("create swapchain: {e}"))?;
        let images = unsafe { loader.get_swapchain_images(chain) }.map_err(|e| e.to_string())?;
        let semaphore = || unsafe { gpu.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }.unwrap();
        let release = images.iter().map(|_| semaphore()).collect();
        Ok(Swapchain { surface_loader, loader, surface, chain, images, extent, acquire: semaphore(), release })
    }

    fn destroy(self, gpu: &Gpu) {
        unsafe {
            gpu.device.device_wait_idle().ok();
            for s in self.release {
                gpu.device.destroy_semaphore(s, None);
            }
            gpu.device.destroy_semaphore(self.acquire, None);
            self.loader.destroy_swapchain(self.chain, None);
            self.surface_loader.destroy_surface(self.surface, None);
        }
    }
}

fn write_png(path: &PathBuf, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("the shot's file");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    // (opaque, as render-check writes its pictures)
    let opaque: Vec<u8> = rgba.chunks(4).flat_map(|c| [c[0], c[1], c[2], 255]).collect();
    enc.write_header().unwrap().write_image_data(&opaque).unwrap();
}

/// An object as the world has it, for comparing two moments of a run.
struct Seen {
    guid: u32,
    place: (i32, i32, i32),
    person: bool,
    data: Vec<i16>,
}

/// The lot's objects (by id) and its clock (globals 0 to 6: the hour, minute and second among
/// them) at a moment.
struct Snapshot {
    objects: std::collections::BTreeMap<i16, Seen>,
    clock: Vec<i16>,
}

impl Snapshot {
    fn of(world: &housefire::world::World) -> Snapshot {
        let objects = world.objects().map(|o| (o.id(), Seen { guid: o.guid, place: (o.x16, o.y16, o.level), person: o.person, data: o.data.clone() })).collect();
        Snapshot { objects, clock: world.globals.iter().take(7).copied().collect() }
    }

    /// What changed from this moment to `now`, in a line (HF_WORLD_DIFF=1: and a line for the
    /// first objects that moved and each whose data changed).
    fn account(&self, now: &Snapshot) -> String {
        let detail = std::env::var_os("HF_WORLD_DIFF").is_some();
        let gone = self.objects.keys().filter(|k| !now.objects.contains_key(k)).count();
        let new = now.objects.keys().filter(|k| !self.objects.contains_key(k)).count();
        let (mut moved, mut persons_moved, mut changed) = (0, 0, 0);
        for (id, a) in &now.objects {
            let Some(b) = self.objects.get(id) else { continue };
            let went = a.place != b.place;
            if went && !a.person && moved < 6 && detail {
                println!("  object {id} guid {:08x}: {:?} then, {:?} now", a.guid, b.place, a.place);
            }
            moved += usize::from(went && !a.person);
            persons_moved += usize::from(went && a.person);
            if a.data != b.data && !a.person {
                if detail {
                    let fields: Vec<(usize, i16, i16)> = a.data.iter().zip(&b.data).enumerate().filter(|(_, (x, y))| x != y).map(|(i, (x, y))| (i, *y, *x)).collect();
                    println!("  object {id} guid {:08x} at {:?}: data (index, then, now) {fields:?}", a.guid, a.place);
                }
                changed += 1;
            }
        }
        format!("{} objects then, {} now ({new} new, {gone} gone); {moved} objects moved, {persons_moved} Sims moved, {changed} objects' data changed; clock globals {:?} then, {:?} now", self.objects.len(), now.objects.len(), self.clock, now.clock)
    }
}

fn main() -> ExitCode {
    let Some(args) = args() else {
        eprintln!("usage: view LOG ISO --card CARD [--lot N] [--ticks N] [--frame-times FILE] [--shot TICK FILE]");
        return ExitCode::from(2);
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("view: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let log = read_log(&args.log).ok_or("cannot read the log")?;
    let disc = Disc::open(&args.iso).map_err(|_| "cannot read the disc image")?;
    let lib = Library::bustin_out(&disc);
    let start = Start::read(&disc, args.card.as_deref(), args.lot)?;
    let house = std::env::var("HF_HOUSE").ok().and_then(|v| v.parse().ok()).unwrap_or(HOME_HOUSE);
    let catalog = housefire::anim::Catalog::read(&disc, house as u32);
    let mut replay = Replay::new(&log, &lib, (start.lot(), start.house.as_ref()), start.globals(), &start.neighbours, None, RunWith { card_lot: start.card_lot, cat: Some(catalog.clone()) }).ok_or("the log does not start with the lot loaded")?;

    // the world as it loads, for the shot's account of what the run changed
    let loaded = Snapshot::of(replay.world());
    // the camera: the log's live parameters at or before a tick (the first record's before the first)
    let first_camera = log.cams.first().map(|c| c.tick).ok_or("the log has no camera records")?;
    let camera_at = |tick: u32| engine_scene::camera(&log, tick.max(first_camera)).expect("a camera record");

    let mut event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let display = event_loop.display_handle().map_err(|e| e.to_string())?.as_raw();
    let mut extensions = surface_extensions(display)?;
    extensions.extend(layer_extensions(&unsafe { ash::Entry::load() }.map_err(|e| format!("no Vulkan loader: {e}"))?));
    let gpu = Gpu::open(std::env::var_os("HF_VALIDATE").is_some(), &extensions, &[ash::khr::swapchain::NAME.as_ptr()])?;
    let mut app = App::default();
    while app.window.is_none() {
        event_loop.pump_app_events(Some(Duration::ZERO), &mut app);
    }
    let window = app.window.as_ref().unwrap();
    let handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
    let surface = make_surface(&gpu, display, handle)?;
    let swapchain = Swapchain::new(&gpu, surface, window.inner_size())?;

    let timestamps_valid = unsafe { gpu.instance.get_physical_device_queue_family_properties(gpu.phys) }[gpu.family as usize].timestamp_valid_bits > 0;
    let period_ms = f64::from(gpu.limits.timestamp_period) / 1e6;
    let pool = unsafe { gpu.device.create_query_pool(&vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2), None) }.map_err(|e| e.to_string())?;
    let mut renderer = Renderer::new(&gpu, WIDTH, HEIGHT);

    let assets = Assets::new(&disc);
    let lot = Lot::new(&disc, house).ok_or("the lot's art")?;
    let mut builder = Builder::new(&assets, &lot);
    // the Sims: the skeletons and the part tables from the disc and the save, their run-time
    // textures from the log's one drawn frame, and whose portrait it shows from that frame
    let ngh = engine_scene::save_file(&disc, args.card.as_deref().map(std::path::Path::new)).ok_or("the save")?;
    let rigs = housefire::figure::Rigs::read(&disc).ok_or("the skeletons")?;
    let cast = housefire::figure::Cast::read(&disc, &ngh).ok_or("the part tables")?;
    let drawn_ticks: std::collections::BTreeSet<u32> = log.strips.iter().map(|s| s.tick).collect();
    let log_scene = drawn_ticks.first().map(|&t| assets.scene(&log, t));
    let skins = log_scene.as_ref().map_or_else(SimSkins::default, SimSkins::from_scene);
    let wardrobe = Wardrobe { rigs: &rigs, cast: &cast, catalog: &catalog, skins: &skins, models: &lot.models, shaders: &lot.shaders };
    let portrait = log_scene.as_ref().and_then(|sc| sims::portrait_sim(sc, &wardrobe, &sims::figures_of(replay.world(), &cast, &[], &Default::default())));
    // (the engine keeps no heading for a Sim apart from its facing: where the log shows a frame,
    // the heading that puts the bones where the log's are is taken, and kept)
    let mut headings = std::collections::HashMap::new();
    let (mut mesh_ids, mut texture_ids) = (Vec::new(), Vec::new());
    let mut times = args.frame_times.as_ref().map(|p| std::io::BufWriter::new(std::fs::File::create(p).expect("the frame-times file")));
    let frame_length = Duration::from_secs_f64(1.0 / TICKS_PER_SECOND);

    let begin = Instant::now();
    let mut previous = begin;
    let mut frames = 0u32;
    let mut presented_tick = 0;
    loop {
        // the frame's time: fixed steps from the first, so that a late one does not move the rest
        let due = begin + frame_length * frames;
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait.saturating_sub(SPIN));
            while Instant::now() < due {
                std::hint::spin_loop();
            }
        }
        let t0 = Instant::now();
        event_loop.pump_app_events(Some(Duration::ZERO), &mut app);
        if app.closed || args.ticks.is_some_and(|n| frames >= n) {
            break;
        }
        // the frame of this tick shows the world as it is when the tick begins (the clock as its
        // passes before it left it), then the tick runs
        let tick = replay.tick() + 1;
        let plays = replay.sim_plays(tick);
        let mut figures = sims::figures_of(replay.world(), &cast, &plays, &headings);
        if drawn_ticks.contains(&tick) {
            headings.extend(sims::headings_from_log(&log, tick, &wardrobe, &figures));
            figures = sims::figures_of(replay.world(), &cast, &plays, &headings);
        }
        let counts = builder.build_with(replay.world(), tick, camera_at(tick), Some((&wardrobe, &figures, portrait)));
        let built = Instant::now();
        let scene = &mut builder.scene;
        if scene.meshes.len() > mesh_ids.len() {
            let ids = renderer.load_meshes(&scene.meshes[mesh_ids.len()..]);
            mesh_ids.extend(ids);
        }
        for levels in &scene.textures[texture_ids.len()..] {
            texture_ids.push(renderer.load_texture(levels));
        }
        // (the transient meshes' ids are the renderer's own already)
        renderer.set_transient_meshes(&scene.transient);
        let mut frame = Frame { camera: scene.frame.camera, draws: std::mem::take(&mut scene.frame.draws), backdrop: scene.frame.backdrop };
        for d in &mut frame.draws {
            if d.mesh.0 & TRANSIENT_MESH == 0 {
                d.mesh = mesh_ids[d.mesh.0 as usize];
            }
            for l in d.material.layers.iter_mut().flatten() {
                l.texture = texture_ids[l.texture.0 as usize];
            }
        }
        if args.shot.as_ref().is_some_and(|s| s.0 == tick) {
            let picture = renderer.render(&frame);
            let (_, path) = args.shot.as_ref().unwrap();
            write_png(path, &picture.rgba);
            let sims = figures.len();
            println!("world at tick {tick} against the loaded save: {}", loaded.account(&Snapshot::of(replay.world())));
            println!("budget at tick {tick}: {:?}", renderer.stats(&frame));
            println!("shot of tick {tick}: {} draws ({} planned, {} floor strips, {} shadows); the {sims} Sims on the lot from the engine's state", counts.draws, counts.planned, counts.floors, counts.shadows);
        }
        let (image, _) = unsafe { swapchain.loader.acquire_next_image(swapchain.chain, u64::MAX, swapchain.acquire, vk::Fence::null()) }.map_err(|e| format!("acquire: {e}"))?;
        let release = swapchain.release[image as usize];
        renderer.draw(&frame, Some(&Present { image: swapchain.images[image as usize], extent: swapchain.extent, wait: swapchain.acquire, signal: release, timestamps: pool }));
        let present = vk::PresentInfoKHR::default().wait_semaphores(std::slice::from_ref(&release)).swapchains(std::slice::from_ref(&swapchain.chain)).image_indices(std::slice::from_ref(&image));
        unsafe { swapchain.loader.queue_present(gpu.queue, &present) }.map_err(|e| format!("present: {e}"))?;
        presented_tick = tick;
        let gpu_ms = if timestamps_valid {
            let mut stamps = [0u64; 2];
            unsafe { gpu.device.get_query_pool_results(pool, 0, &mut stamps, vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT) }.ok();
            stamps[1].saturating_sub(stamps[0]) as f64 * period_ms
        } else {
            f64::NAN
        };
        let drawn = Instant::now();
        let ms = |a: Instant, b: Instant| b.duration_since(a).as_secs_f64() * 1e3;
        let running = replay.step();
        let end = Instant::now();
        if let Some(w) = times.as_mut() {
            writeln!(w, "{tick} {:.3} {:.3} {:.3} {:.3} {:.3} {gpu_ms:.3}", ms(begin, t0), ms(previous, t0), ms(drawn, end), ms(t0, built), ms(t0, end)).ok();
        }
        previous = t0;
        frames += 1;
        if !running {
            break;
        }
    }
    println!("{frames} frames, the last of tick {presented_tick}; {} meshes and {} textures on the GPU", mesh_ids.len(), texture_ids.len());
    drop(builder);
    unsafe { gpu.device.device_wait_idle().ok() };
    unsafe { gpu.device.destroy_query_pool(pool, None) };
    swapchain.destroy(&gpu);
    drop(renderer);
    Ok(())
}
