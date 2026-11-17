//! A headless Vulkan device (raw `ash`): instance, one graphics queue, memory and one-shot
//! command helpers. No window system: pictures are rendered into images and read back.

use ash::vk;
use std::ffi::{CStr, c_char};

pub struct Gpu {
    /// The loader, kept alive for as long as the device (and used for the window system's extensions).
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub phys: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub family: u32,
    pub pool: vk::CommandPool,
    memory: vk::PhysicalDeviceMemoryProperties,
    pub limits: vk::PhysicalDeviceLimits,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
}

/// A buffer with its own memory.
pub struct Buffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: u64,
}

/// An image with its own memory and a view of all its levels.
pub struct Image {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
}

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _kind: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let msg = unsafe { CStr::from_ptr((*data).p_message) };
    eprintln!("[vulkan {severity:?}] {}", msg.to_string_lossy());
    vk::FALSE
}

impl Gpu {
    /// Opens the first device with a graphics queue (the discrete one if there is one). With
    /// `validate` the Khronos validation layer is on when installed.
    pub fn new(validate: bool) -> Result<Gpu, String> {
        Gpu::open(validate, &[], &[])
    }

    /// As `new`, with instance and device extensions on (a window's surface and swapchain).
    pub fn open(validate: bool, instance_extensions: &[*const c_char], device_extensions: &[*const c_char]) -> Result<Gpu, String> {
        let entry = unsafe { ash::Entry::load() }.map_err(|e| format!("no Vulkan loader: {e}"))?;
        let app = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 1, 0));
        let layer = c"VK_LAYER_KHRONOS_validation";
        let have_layer = validate
            && unsafe { entry.enumerate_instance_layer_properties() }
                .map_err(|e| e.to_string())?
                .iter()
                .any(|l| l.layer_name_as_c_str().is_ok_and(|n| n == layer));
        let layers: Vec<*const c_char> = if have_layer { vec![layer.as_ptr()] } else { vec![] };
        let mut exts: Vec<*const c_char> = if have_layer { vec![ash::ext::debug_utils::NAME.as_ptr()] } else { vec![] };
        exts.extend_from_slice(instance_extensions);
        let info = vk::InstanceCreateInfo::default().application_info(&app).enabled_layer_names(&layers).enabled_extension_names(&exts);
        let instance = unsafe { entry.create_instance(&info, None) }.map_err(|e| format!("create_instance: {e}"))?;
        let debug = have_layer.then(|| {
            let du = ash::ext::debug_utils::Instance::new(&entry, &instance);
            let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR)
                .message_type(vk::DebugUtilsMessageTypeFlagsEXT::GENERAL | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE)
                .pfn_user_callback(Some(debug_callback));
            let m = unsafe { du.create_debug_utils_messenger(&info, None) }.unwrap();
            (du, m)
        });
        let phys_all = unsafe { instance.enumerate_physical_devices() }.map_err(|e| e.to_string())?;
        let pick = phys_all
            .iter()
            .copied()
            .filter_map(|p| {
                let fam = unsafe { instance.get_physical_device_queue_family_properties(p) }.iter().position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))?;
                let t = unsafe { instance.get_physical_device_properties(p) }.device_type;
                Some((u32::from(t != vk::PhysicalDeviceType::DISCRETE_GPU), p, fam as u32))
            })
            .min_by_key(|x| x.0)
            .ok_or("no graphics device")?;
        let (phys, family) = (pick.1, pick.2);
        let prio = [1.0f32];
        let qi = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&prio)];
        let features = vk::PhysicalDeviceFeatures::default().independent_blend(true);
        let device = unsafe { instance.create_device(phys, &vk::DeviceCreateInfo::default().queue_create_infos(&qi).enabled_features(&features).enabled_extension_names(device_extensions), None) }.map_err(|e| format!("create_device: {e}"))?;
        let queue = unsafe { device.get_device_queue(family, 0) };
        let pool = unsafe { device.create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(family).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None) }.map_err(|e| e.to_string())?;
        let memory = unsafe { instance.get_physical_device_memory_properties(phys) };
        let limits = unsafe { instance.get_physical_device_properties(phys) }.limits;
        Ok(Gpu { entry, instance, phys, device, queue, family, pool, memory, limits, debug })
    }

    pub fn memory_type(&self, bits: u32, want: vk::MemoryPropertyFlags) -> u32 {
        (0..self.memory.memory_type_count).find(|&i| bits & (1 << i) != 0 && self.memory.memory_types[i as usize].property_flags.contains(want)).expect("no memory type")
    }

    pub fn buffer(&self, size: u64, usage: vk::BufferUsageFlags, props: vk::MemoryPropertyFlags) -> Buffer {
        let size = size.max(16);
        let buffer = unsafe { self.device.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(usage), None) }.unwrap();
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let memory = unsafe { self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(self.memory_type(req.memory_type_bits, props)), None) }.unwrap();
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0) }.unwrap();
        Buffer { buffer, memory, size }
    }

    /// A host-visible buffer holding `data`.
    pub fn staging(&self, data: &[u8]) -> Buffer {
        let b = self.buffer(data.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT);
        self.write(&b, 0, data);
        b
    }

    pub fn write(&self, b: &Buffer, offset: u64, data: &[u8]) {
        unsafe {
            let p = self.device.map_memory(b.memory, offset, data.len().max(1) as u64, vk::MemoryMapFlags::empty()).unwrap();
            std::ptr::copy_nonoverlapping(data.as_ptr(), p.cast::<u8>(), data.len());
            self.device.unmap_memory(b.memory);
        }
    }

    pub fn read(&self, b: &Buffer, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        unsafe {
            let p = self.device.map_memory(b.memory, 0, len as u64, vk::MemoryMapFlags::empty()).unwrap();
            std::ptr::copy_nonoverlapping(p.cast::<u8>(), out.as_mut_ptr(), len);
            self.device.unmap_memory(b.memory);
        }
        out
    }

    /// A device-local buffer with `data` in it (through a staging buffer, waited for).
    pub fn upload(&self, data: &[u8], usage: vk::BufferUsageFlags) -> Buffer {
        let dst = self.buffer(data.len() as u64, usage | vk::BufferUsageFlags::TRANSFER_DST, vk::MemoryPropertyFlags::DEVICE_LOCAL);
        let src = self.staging(data);
        self.one_shot(|cb| unsafe { self.device.cmd_copy_buffer(cb, src.buffer, dst.buffer, &[vk::BufferCopy::default().size(data.len() as u64)]) });
        self.destroy_buffer(src);
        dst
    }

    pub fn destroy_buffer(&self, b: Buffer) {
        unsafe {
            self.device.destroy_buffer(b.buffer, None);
            self.device.free_memory(b.memory, None);
        }
    }

    pub fn image(&self, info: &vk::ImageCreateInfo, aspect: vk::ImageAspectFlags) -> Image {
        let image = unsafe { self.device.create_image(info, None) }.unwrap();
        let req = unsafe { self.device.get_image_memory_requirements(image) };
        let memory = unsafe { self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(self.memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)), None) }.unwrap();
        unsafe { self.device.bind_image_memory(image, memory, 0) }.unwrap();
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(info.format)
            .subresource_range(vk::ImageSubresourceRange::default().aspect_mask(aspect).level_count(info.mip_levels).layer_count(1));
        let view = unsafe { self.device.create_image_view(&view_info, None) }.unwrap();
        Image { image, memory, view }
    }

    pub fn destroy_image(&self, i: Image) {
        unsafe {
            self.device.destroy_image_view(i.view, None);
            self.device.destroy_image(i.image, None);
            self.device.free_memory(i.memory, None);
        }
    }

    /// Records commands with `f`, submits them and waits (loading time only).
    pub fn one_shot(&self, f: impl FnOnce(vk::CommandBuffer)) {
        let cb = unsafe { self.device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(self.pool).command_buffer_count(1)) }.unwrap()[0];
        unsafe {
            self.device.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).unwrap();
            f(cb);
            self.device.end_command_buffer(cb).unwrap();
            let fence = self.device.create_fence(&vk::FenceCreateInfo::default(), None).unwrap();
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::default().command_buffers(&[cb])], fence).unwrap();
            // bounded: a hung GPU gives up after ten seconds instead of waiting for ever
            let waited = self.device.wait_for_fences(&[fence], true, 10_000_000_000);
            assert!(waited.is_ok(), "the GPU did not finish within 10 s");
            self.device.destroy_fence(fence, None);
            self.device.free_command_buffers(self.pool, &[cb]);
        }
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
            if let Some((du, m)) = self.debug.take() {
                du.destroy_debug_utils_messenger(m, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}
