//! A window and its swapchain for the `play` binary (the same setup as `view`'s: Wayland or X11,
//! Vulkan surface, FIFO, the picture blitted to the swapchain image).

use crate::gpu::Gpu;
use ash::vk;
use std::path::Path;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

/// The window, as the event loop makes it, and the events it has received since they were taken.
pub struct App {
    pub window: Option<Window>,
    pub closed: bool,
    pub events: Vec<WindowEvent>,
    title: &'static str,
    size: PhysicalSize<u32>,
}

impl App {
    pub fn new(title: &'static str, width: u32, height: u32) -> App {
        App { window: None, closed: false, events: Vec::new(), title, size: PhysicalSize::new(width, height) }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = Window::default_attributes().with_title(self.title).with_inner_size(self.size).with_resizable(false);
            self.window = Some(event_loop.create_window(attributes).expect("a window"));
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.closed = true;
        }
        self.events.push(event);
    }
}

/// The instance extensions the window system's surfaces need.
pub fn surface_extensions(display: RawDisplayHandle) -> Result<Vec<*const std::ffi::c_char>, String> {
    let platform = match display {
        RawDisplayHandle::Wayland(_) => ash::khr::wayland_surface::NAME,
        RawDisplayHandle::Xlib(_) => ash::khr::xlib_surface::NAME,
        RawDisplayHandle::Xcb(_) => ash::khr::xcb_surface::NAME,
        d => return Err(format!("no surface for {d:?}")),
    };
    Ok(vec![ash::khr::surface::NAME.as_ptr(), platform.as_ptr()])
}

/// The instance extensions gamescope's window-system layer wants besides (it forces
/// VK_EXT_swapchain_maintenance1 on the device): only those the loader lists.
pub fn layer_extensions(entry: &ash::Entry) -> Vec<*const std::ffi::c_char> {
    let have = unsafe { entry.enumerate_instance_extension_properties(None) }.unwrap_or_default();
    let listed = |name: &std::ffi::CStr| have.iter().any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name));
    let wanted = [ash::ext::surface_maintenance1::NAME, ash::khr::get_surface_capabilities2::NAME];
    if wanted.iter().all(|n| listed(n)) { wanted.iter().map(|n| n.as_ptr()).collect() } else { Vec::new() }
}

pub fn make_surface(gpu: &Gpu, display: RawDisplayHandle, window: RawWindowHandle) -> Result<vk::SurfaceKHR, String> {
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
pub struct Swapchain {
    surface_loader: ash::khr::surface::Instance,
    pub loader: ash::khr::swapchain::Device,
    surface: vk::SurfaceKHR,
    pub chain: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub extent: vk::Extent2D,
    pub acquire: vk::Semaphore,
    pub release: Vec<vk::Semaphore>,
}

impl Swapchain {
    pub fn new(gpu: &Gpu, surface: vk::SurfaceKHR, window: PhysicalSize<u32>) -> Result<Swapchain, String> {
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

    pub fn destroy(self, gpu: &Gpu) {
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

/// A picture (RGBA, opaque) saved as PNG.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("the shot's file");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let opaque: Vec<u8> = rgba.chunks(4).flat_map(|c| [c[0], c[1], c[2], 255]).collect();
    enc.write_header().unwrap().write_image_data(&opaque).unwrap();
}
