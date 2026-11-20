//! The renderer's interface pass: a picture the size of the frame (premultiplied RGBA, made on
//! the CPU by `overlay::Overlay`) laid over the colour image after the scene, before the picture
//! is read back or presented. It knows nothing of what the picture shows, so the original's own
//! interface can take the place of the stand-in one without touching it.

use crate::gpu::{Buffer, Gpu, Image};
use ash::vk;

pub struct OverlayPass<'g> {
    gpu: &'g Gpu,
    width: u32,
    height: u32,
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    image: Image,
    staging: Buffer,
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    sampler: vk::Sampler,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    vert: vk::ShaderModule,
    frag: vk::ShaderModule,
}

impl<'g> OverlayPass<'g> {
    /// The pass over `colour` (the renderer's colour image, `format`, left in the transfer-source
    /// layout by the scene pass and in it again by this one).
    pub fn new(gpu: &'g Gpu, colour: vk::ImageView, format: vk::Format, width: u32, height: u32) -> OverlayPass<'g> {
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
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
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
        let staging = gpu.buffer(u64::from(width * height * 4), vk::BufferUsageFlags::TRANSFER_SRC, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT);
        let sampler = unsafe { d.create_sampler(&vk::SamplerCreateInfo::default().mag_filter(vk::Filter::NEAREST).min_filter(vk::Filter::NEAREST).address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE).address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE), None) }.unwrap();
        let binding = [vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let set_layout = unsafe { d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding), None) }.unwrap();
        let sizes = [vk::DescriptorPoolSize::default().ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1)];
        let pool = unsafe { d.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(1).pool_sizes(&sizes), None) }.unwrap();
        let layouts = [set_layout];
        let set = unsafe { d.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&layouts)) }.unwrap()[0];
        let info = [vk::DescriptorImageInfo::default().sampler(sampler).image_view(image.view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        unsafe { d.update_descriptor_sets(&[vk::WriteDescriptorSet::default().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&info)], &[]) };
        let pipeline_layout = unsafe { d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts), None) }.unwrap();
        let module = |spv: &[u8]| {
            let words: Vec<u32> = spv.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
            unsafe { d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None) }.unwrap()
        };
        let vert = module(include_bytes!(concat!(env!("OUT_DIR"), "/overlay.vert.spv")));
        let frag = module(include_bytes!(concat!(env!("OUT_DIR"), "/overlay.frag.spv")));
        let stages = [
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vert).name(c"main"),
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(frag).name(c"main"),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewports = [vk::Viewport { x: 0.0, y: 0.0, width: width as f32, height: height as f32, min_depth: 0.0, max_depth: 1.0 }];
        let scissors = [vk::Rect2D { offset: vk::Offset2D::default(), extent: vk::Extent2D { width, height } }];
        let viewport = vk::PipelineViewportStateCreateInfo::default().viewports(&viewports).scissors(&scissors);
        let raster = vk::PipelineRasterizationStateCreateInfo::default().polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        // premultiplied: the overlay's colour already carries its alpha
        let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)];
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
        OverlayPass { gpu, width, height, render_pass, framebuffer, image, staging, set_layout, pool, set, sampler, pipeline_layout, pipeline, vert, frag }
    }

    /// Takes the next frame's picture (`width * height * 4` bytes of premultiplied RGBA).
    pub fn set(&self, rgba: &[u8]) {
        assert_eq!(rgba.len() as u64, u64::from(self.width * self.height * 4), "the overlay is the frame's size");
        self.gpu.write(&self.staging, 0, rgba);
    }

    /// Records the upload of the picture and the pass that lays it over the colour image (which
    /// the scene pass has left in the transfer-source layout).
    pub fn record(&self, cb: vk::CommandBuffer) {
        let d = &self.gpu.device;
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let barrier = |from: vk::ImageLayout, to: vk::ImageLayout, src: vk::AccessFlags, dst: vk::AccessFlags| vk::ImageMemoryBarrier::default().old_layout(from).new_layout(to).src_access_mask(src).dst_access_mask(dst).image(self.image.image).subresource_range(range);
        unsafe {
            d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_WRITE)]);
            let region = vk::BufferImageCopy::default().image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1)).image_extent(vk::Extent3D { width: self.width, height: self.height, depth: 1 });
            d.cmd_copy_buffer_to_image(cb, self.staging.buffer, self.image.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
            d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[barrier(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::AccessFlags::TRANSFER_WRITE, vk::AccessFlags::SHADER_READ)]);
            d.cmd_begin_render_pass(
                cb,
                &vk::RenderPassBeginInfo::default().render_pass(self.render_pass).framebuffer(self.framebuffer).render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent: vk::Extent2D { width: self.width, height: self.height } }),
                vk::SubpassContents::INLINE,
            );
            d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0, &[self.set], &[]);
            d.cmd_draw(cb, 3, 1, 0, 0);
            d.cmd_end_render_pass(cb);
        }
    }
}

impl Drop for OverlayPass<'_> {
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
        // (the staging buffer and the image are released by their owners' drop below)
        let staging = std::mem::replace(&mut self.staging, Buffer { buffer: vk::Buffer::null(), memory: vk::DeviceMemory::null(), size: 0 });
        gpu.destroy_buffer(staging);
        let image = std::mem::replace(&mut self.image, Image { image: vk::Image::null(), memory: vk::DeviceMemory::null(), view: vk::ImageView::null() });
        gpu.destroy_image(image);
    }
}
