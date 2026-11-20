//! The movies' pass: the Apt player's display list, drawn as triangles with a solid colour or a
//! texture and a colour transform over the colour image (after the scene, before the picture is
//! read back). Positions come in clip space, so the picture can be any size: the drawing
//! (`apt_draw`) scales the movies' stage to it. Textures are uploaded once (`add_texture`) and
//! kept for as long as the pass lives.

use crate::gpu::{Buffer, Gpu, Image};
use ash::vk;

/// One corner of a triangle, 18 floats of 4 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    /// Clip space.
    pub pos: [f32; 2],
    /// Texture coordinates, 0 to 1 over the whole texture.
    pub uv: [f32; 2],
    /// The fill's colour, which the texel is multiplied by.
    pub tint: [f32; 4],
    /// The clip's colour transform: multiplier and offset.
    pub mul: [f32; 4],
    pub add: [f32; 4],
    /// 1 when the texture holds premultiplied colours (the fonts' pages), else 0.
    pub premultiplied: f32,
    pub _pad: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureId(pub u32);

/// A run of vertices (whole triangles) drawn with one texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    pub texture: TextureId,
    pub first: u32,
    pub count: u32,
}

/// A colour image of `width` by `height` (RGBA, unsigned normalised) the pass can draw over and
/// be read back from: for pictures made offscreen (the checks).
pub fn target(gpu: &Gpu, width: u32, height: u32) -> Image {
    gpu.image(
        &vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(TARGET_FORMAT)
            .extent(vk::Extent3D { width, height, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST),
        vk::ImageAspectFlags::COLOR,
    )
}

/// The format of `target`.
pub const TARGET_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// Puts `base` (`width * height * 4` bytes of RGBA) in `colour` (a `target`), draws the pass over
/// it and reads the picture back.
pub fn render_over(gpu: &Gpu, pass: &AptPass, colour: &Image, base: &[u8], width: u32, height: u32) -> Vec<u8> {
    let d = &gpu.device;
    let staging = gpu.staging(base);
    let size = (width * height * 4) as usize;
    let out = gpu.buffer(size as u64, vk::BufferUsageFlags::TRANSFER_DST, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT);
    let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
    let barrier = |from, to, src, dst| vk::ImageMemoryBarrier::default().old_layout(from).new_layout(to).src_access_mask(src).dst_access_mask(dst).image(colour.image).subresource_range(range);
    let region = vk::BufferImageCopy::default().image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1)).image_extent(vk::Extent3D { width, height, depth: 1 });
    gpu.one_shot(|cb| unsafe {
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_WRITE)]);
        d.cmd_copy_buffer_to_image(cb, staging.buffer, colour.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, vk::AccessFlags::TRANSFER_WRITE, vk::AccessFlags::TRANSFER_READ)]);
        pass.record(cb);
        d.cmd_copy_image_to_buffer(cb, colour.image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, out.buffer, &[region]);
    });
    let rgba = gpu.read(&out, size);
    gpu.destroy_buffer(staging);
    gpu.destroy_buffer(out);
    rgba
}

/// Textures a pass can hold (one descriptor set each).
const MAX_TEXTURES: u32 = 4096;

pub struct AptPass<'g> {
    gpu: &'g Gpu,
    width: u32,
    height: u32,
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    vert: vk::ShaderModule,
    frag: vk::ShaderModule,
    textures: Vec<(Image, vk::DescriptorSet)>,
    vertices: Option<Buffer>,
    batches: Vec<Batch>,
}

impl<'g> AptPass<'g> {
    /// The pass over `colour` (an image of `format` that the scene pass leaves in the
    /// transfer-source layout, and this one leaves in it again). `linear` picks bilinear
    /// filtering for the textures, otherwise the nearest texel.
    pub fn new(gpu: &'g Gpu, colour: vk::ImageView, format: vk::Format, width: u32, height: u32, linear: bool) -> AptPass<'g> {
        let d = &gpu.device;
        let attachment = [vk::AttachmentDescription::default()
            .format(format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)];
        let refs = [vk::AttachmentReference::default().attachment(0).layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let subpass = [vk::SubpassDescription::default().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&refs)];
        let deps = [
            vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::TRANSFER)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
            vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ),
        ];
        let render_pass = unsafe { d.create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&attachment).subpasses(&subpass).dependencies(&deps), None) }.unwrap();
        let views = [colour];
        let framebuffer = unsafe { d.create_framebuffer(&vk::FramebufferCreateInfo::default().render_pass(render_pass).attachments(&views).width(width).height(height).layers(1), None) }.unwrap();
        let filter = if linear { vk::Filter::LINEAR } else { vk::Filter::NEAREST };
        let sampler = unsafe {
            d.create_sampler(
                &vk::SamplerCreateInfo::default().mag_filter(filter).min_filter(filter).address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE).address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )
        }
        .unwrap();
        let binding = [vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let set_layout = unsafe { d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding), None) }.unwrap();
        let sizes = [vk::DescriptorPoolSize::default().ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(MAX_TEXTURES)];
        let pool = unsafe { d.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(MAX_TEXTURES).pool_sizes(&sizes), None) }.unwrap();
        let layouts = [set_layout];
        let pipeline_layout = unsafe { d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts), None) }.unwrap();
        let module = |spv: &[u8]| {
            let words: Vec<u32> = spv.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
            unsafe { d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None) }.unwrap()
        };
        let vert = module(include_bytes!(concat!(env!("OUT_DIR"), "/apt.vert.spv")));
        let frag = module(include_bytes!(concat!(env!("OUT_DIR"), "/apt.frag.spv")));
        let stages = [
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vert).name(c"main"),
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(frag).name(c"main"),
        ];
        let bindings = [vk::VertexInputBindingDescription::default().binding(0).stride(std::mem::size_of::<Vertex>() as u32).input_rate(vk::VertexInputRate::VERTEX)];
        let attributes = [
            vk::VertexInputAttributeDescription::default().location(0).format(vk::Format::R32G32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default().location(1).format(vk::Format::R32G32_SFLOAT).offset(8),
            vk::VertexInputAttributeDescription::default().location(2).format(vk::Format::R32G32B32A32_SFLOAT).offset(16),
            vk::VertexInputAttributeDescription::default().location(3).format(vk::Format::R32G32B32A32_SFLOAT).offset(32),
            vk::VertexInputAttributeDescription::default().location(4).format(vk::Format::R32G32B32A32_SFLOAT).offset(48),
            vk::VertexInputAttributeDescription::default().location(5).format(vk::Format::R32_SFLOAT).offset(64),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&bindings).vertex_attribute_descriptions(&attributes);
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewports = [vk::Viewport { x: 0.0, y: 0.0, width: width as f32, height: height as f32, min_depth: 0.0, max_depth: 1.0 }];
        let scissors = [vk::Rect2D { offset: vk::Offset2D::default(), extent: vk::Extent2D { width, height } }];
        let viewport = vk::PipelineViewportStateCreateInfo::default().viewports(&viewports).scissors(&scissors);
        let raster = vk::PipelineRasterizationStateCreateInfo::default().polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        // the shader gives premultiplied colour; the picture's own alpha stays
        let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .src_alpha_blend_factor(vk::BlendFactor::ZERO)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
        let info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&blend)
            .layout(pipeline_layout)
            .render_pass(render_pass)];
        let pipeline = unsafe { d.create_graphics_pipelines(vk::PipelineCache::null(), &info, None) }.map_err(|e| e.1).unwrap()[0];
        AptPass { gpu, width, height, render_pass, framebuffer, set_layout, pool, sampler, pipeline_layout, pipeline, vert, frag, textures: Vec::new(), vertices: None, batches: Vec::new() }
    }

    /// Uploads a picture (`width * height * 4` bytes of straight RGBA, from the top) once.
    pub fn add_texture(&mut self, width: u32, height: u32, rgba: &[u8]) -> TextureId {
        assert_eq!(rgba.len() as u64, u64::from(width) * u64::from(height) * 4, "a texture is RGBA");
        assert!((self.textures.len() as u32) < MAX_TEXTURES, "too many textures");
        let (gpu, d) = (self.gpu, &self.gpu.device);
        let image = gpu.image(
            &vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_UNORM)
                .extent(vk::Extent3D { width, height, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST),
            vk::ImageAspectFlags::COLOR,
        );
        let staging = gpu.staging(rgba);
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let barrier = |from, to, src, dst| vk::ImageMemoryBarrier::default().old_layout(from).new_layout(to).src_access_mask(src).dst_access_mask(dst).image(image.image).subresource_range(range);
        gpu.one_shot(|cb| unsafe {
            d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_WRITE)]);
            let region = vk::BufferImageCopy::default().image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1)).image_extent(vk::Extent3D { width, height, depth: 1 });
            d.cmd_copy_buffer_to_image(cb, staging.buffer, image.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
            d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::AccessFlags::TRANSFER_WRITE, vk::AccessFlags::SHADER_READ)]);
        });
        gpu.destroy_buffer(staging);
        let layouts = [self.set_layout];
        let set = unsafe { d.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.pool).set_layouts(&layouts)) }.unwrap()[0];
        let info = [vk::DescriptorImageInfo::default().sampler(self.sampler).image_view(image.view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        unsafe { d.update_descriptor_sets(&[vk::WriteDescriptorSet::default().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&info)], &[]) };
        self.textures.push((image, set));
        TextureId(self.textures.len() as u32 - 1)
    }

    /// Takes the next frame's triangles and their runs (waits for the last frame's use of the
    /// buffer, so call it between frames).
    pub fn set(&mut self, vertices: &[Vertex], batches: &[Batch]) {
        let bytes = std::mem::size_of_val(vertices) as u64;
        if self.vertices.as_ref().is_none_or(|b| b.size < bytes) {
            unsafe { self.gpu.device.device_wait_idle().ok() };
            if let Some(old) = self.vertices.take() {
                self.gpu.destroy_buffer(old);
            }
            let size = bytes.max(1 << 16).next_power_of_two();
            self.vertices = Some(self.gpu.buffer(size, vk::BufferUsageFlags::VERTEX_BUFFER, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT));
        }
        // SAFETY: `Vertex` is plain floats with no padding.
        let raw = unsafe { std::slice::from_raw_parts(vertices.as_ptr().cast::<u8>(), bytes as usize) };
        if let Some(b) = &self.vertices {
            self.gpu.write(b, 0, raw);
        }
        self.batches.clear();
        self.batches.extend_from_slice(batches);
    }

    /// Records the pass over the colour image.
    pub fn record(&self, cb: vk::CommandBuffer) {
        let Some(vertices) = &self.vertices else { return };
        let d = &self.gpu.device;
        unsafe {
            d.cmd_begin_render_pass(
                cb,
                &vk::RenderPassBeginInfo::default().render_pass(self.render_pass).framebuffer(self.framebuffer).render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent: vk::Extent2D { width: self.width, height: self.height } }),
                vk::SubpassContents::INLINE,
            );
            d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            d.cmd_bind_vertex_buffers(cb, 0, &[vertices.buffer], &[0]);
            for b in &self.batches {
                d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0, &[self.textures[b.texture.0 as usize].1], &[]);
                d.cmd_draw(cb, b.count, 1, b.first, 0);
            }
            d.cmd_end_render_pass(cb);
        }
    }
}

impl Drop for AptPass<'_> {
    fn drop(&mut self) {
        let gpu = self.gpu;
        let d = &gpu.device;
        unsafe {
            d.device_wait_idle().ok();
            d.destroy_pipeline(self.pipeline, None);
            d.destroy_pipeline_layout(self.pipeline_layout, None);
            d.destroy_shader_module(self.vert, None);
            d.destroy_shader_module(self.frag, None);
            d.destroy_descriptor_pool(self.pool, None);
            d.destroy_descriptor_set_layout(self.set_layout, None);
            d.destroy_sampler(self.sampler, None);
            d.destroy_framebuffer(self.framebuffer, None);
            d.destroy_render_pass(self.render_pass, None);
        }
        for (image, _) in self.textures.drain(..) {
            gpu.destroy_image(image);
        }
        if let Some(b) = self.vertices.take() {
            gpu.destroy_buffer(b);
        }
    }
}
