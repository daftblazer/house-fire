//! The renderer: meshes and textures uploaded once, then a frame's draws recorded into one
//! command buffer and rendered offscreen. Per-draw numbers (matrices, light set, material
//! switches) are one storage buffer indexed by the instance; no vertex is touched on the CPU.

use crate::frame::*;
use crate::gpu::{Buffer, Gpu, Image};
use ash::vk;
use std::collections::HashMap;

const COLOUR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const ID_FORMAT: vk::Format = vk::Format::R32_UINT;
const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

/// The per-draw record the shaders read (`shaders/model.vert`, std430: all vec4s).
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct DrawRecord {
    mv: [[f32; 4]; 3],
    nm: [[f32; 4]; 3],
    ambient: [f32; 4],
    dir_colour: [[f32; 4]; MAX_DIRECTIONALS],
    dir_to_light: [[f32; 4]; MAX_DIRECTIONALS],
    point_colour: [[f32; 4]; MAX_POINTS],
    point_position: [[f32; 4]; MAX_POINTS],
    counts: [i32; 4],
    params: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    blend: Blend,
    cull: Cull,
    depth_test: bool,
    depth_write: bool,
}

struct MeshSet {
    vertices: Buffer,
    indices: Buffer,
}

#[derive(Clone, Copy)]
struct MeshSlot {
    set: usize,
    first_index: u32,
    index_count: u32,
    vertex_offset: i32,
}

struct Tex {
    image: Image,
}

pub struct Renderer<'g> {
    gpu: &'g Gpu,
    width: u32,
    height: u32,
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    colour: Image,
    ids: Image,
    depth: Image,
    colour_out: Buffer,
    ids_out: Buffer,
    draw_layout: vk::DescriptorSetLayout,
    tex_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    vert: vk::ShaderModule,
    frag: vk::ShaderModule,
    pipelines: HashMap<PipelineKey, vk::Pipeline>,
    samplers: HashMap<(Wrap, Wrap, bool, bool), vk::Sampler>,
    pool: vk::DescriptorPool,
    draw_set: vk::DescriptorSet,
    draw_buffer: Buffer,
    draw_capacity: usize,
    tex_sets: HashMap<[Option<TextureRef>; 2], vk::DescriptorSet>,
    meshes: Vec<MeshSlot>,
    mesh_sets: Vec<MeshSet>,
    textures: Vec<Tex>,
    white: TextureId,
}

fn bytes_of<T: Copy>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), std::mem::size_of_val(v)) }
}

impl<'g> Renderer<'g> {
    pub fn new(gpu: &'g Gpu, width: u32, height: u32) -> Renderer<'g> {
        let d = &gpu.device;
        let attachments = [
            attachment(COLOUR_FORMAT, vk::ImageLayout::TRANSFER_SRC_OPTIMAL),
            attachment(ID_FORMAT, vk::ImageLayout::TRANSFER_SRC_OPTIMAL),
            attachment(DEPTH_FORMAT, vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
        ];
        let colour_refs = [
            vk::AttachmentReference::default().attachment(0).layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
            vk::AttachmentReference::default().attachment(1).layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
        ];
        let depth_ref = vk::AttachmentReference::default().attachment(2).layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
        let subpass = [vk::SubpassDescription::default().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&colour_refs).depth_stencil_attachment(&depth_ref)];
        let deps = [vk::SubpassDependency::default()
            .src_subpass(0)
            .dst_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)];
        let render_pass = unsafe { d.create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&attachments).subpasses(&subpass).dependencies(&deps), None) }.unwrap();

        let target = |format, usage| {
            gpu.image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D { width, height, depth: 1 })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .usage(usage),
                if format == DEPTH_FORMAT { vk::ImageAspectFlags::DEPTH } else { vk::ImageAspectFlags::COLOR },
            )
        };
        let colour = target(COLOUR_FORMAT, vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC);
        let ids = target(ID_FORMAT, vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC);
        let depth = target(DEPTH_FORMAT, vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT);
        let views = [colour.view, ids.view, depth.view];
        let framebuffer = unsafe { d.create_framebuffer(&vk::FramebufferCreateInfo::default().render_pass(render_pass).attachments(&views).width(width).height(height).layers(1), None) }.unwrap();
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let colour_out = gpu.buffer(u64::from(width * height * 4), vk::BufferUsageFlags::TRANSFER_DST, host);
        let ids_out = gpu.buffer(u64::from(width * height * 4), vk::BufferUsageFlags::TRANSFER_DST, host);

        let draw_layout = unsafe {
            d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)]),
                None,
            )
        }
        .unwrap();
        let tex_bindings = [0, 1].map(|b| vk::DescriptorSetLayoutBinding::default().binding(b).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT));
        let tex_layout = unsafe { d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&tex_bindings), None) }.unwrap();
        let layouts = [draw_layout, tex_layout];
        let push = [vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::VERTEX).size(32)];
        let pipeline_layout = unsafe { d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts).push_constant_ranges(&push), None) }.unwrap();
        let module = |spv: &[u8]| {
            let words: Vec<u32> = spv.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            unsafe { d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None) }.unwrap()
        };
        let vert = module(include_bytes!(concat!(env!("OUT_DIR"), "/model.vert.spv")));
        let frag = module(include_bytes!(concat!(env!("OUT_DIR"), "/model.frag.spv")));
        let pool_sizes = [
            vk::DescriptorPoolSize::default().ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(8192),
            vk::DescriptorPoolSize::default().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(4),
        ];
        let pool = unsafe { d.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(4100).pool_sizes(&pool_sizes), None) }.unwrap();
        let draw_set = unsafe { d.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&[draw_layout])) }.unwrap()[0];
        let draw_capacity = 4096;
        let draw_buffer = gpu.buffer((draw_capacity * std::mem::size_of::<DrawRecord>()) as u64, vk::BufferUsageFlags::STORAGE_BUFFER, host);
        let mut r = Renderer {
            gpu,
            width,
            height,
            render_pass,
            framebuffer,
            colour,
            ids,
            depth,
            colour_out,
            ids_out,
            draw_layout,
            tex_layout,
            pipeline_layout,
            vert,
            frag,
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            pool,
            draw_set,
            draw_buffer,
            draw_capacity,
            tex_sets: HashMap::new(),
            meshes: Vec::new(),
            mesh_sets: Vec::new(),
            textures: Vec::new(),
            white: TextureId(0),
        };
        r.bind_draw_buffer();
        r.white = r.load_texture(&[(1, 1, vec![0xFFFF_FFFF])]);
        r
    }

    fn bind_draw_buffer(&self) {
        let info = [vk::DescriptorBufferInfo::default().buffer(self.draw_buffer.buffer).range(vk::WHOLE_SIZE)];
        let write = [vk::WriteDescriptorSet::default().dst_set(self.draw_set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&info)];
        unsafe { self.gpu.device.update_descriptor_sets(&write, &[]) };
    }

    /// Uploads meshes into one vertex and one index buffer, once; the ids come back in order.
    pub fn load_meshes(&mut self, meshes: &[MeshData]) -> Vec<MeshId> {
        let set = self.mesh_sets.len();
        let (mut vertices, mut indices) = (Vec::new(), Vec::<u32>::new());
        let mut ids = Vec::new();
        for m in meshes {
            ids.push(MeshId(self.meshes.len() as u32));
            self.meshes.push(MeshSlot { set, first_index: indices.len() as u32, index_count: m.indices.len() as u32, vertex_offset: vertices.len() as i32 });
            vertices.extend_from_slice(&m.vertices);
            indices.extend_from_slice(&m.indices);
        }
        self.mesh_sets.push(MeshSet {
            vertices: self.gpu.upload(bytes_of(&vertices), vk::BufferUsageFlags::VERTEX_BUFFER),
            indices: self.gpu.upload(bytes_of(&indices), vk::BufferUsageFlags::INDEX_BUFFER),
        });
        ids
    }

    /// Uploads a texture, once: its levels (width, height, RGBA texels with red in the low
    /// byte, largest first).
    pub fn load_texture(&mut self, levels: &[(u32, u32, Vec<u32>)]) -> TextureId {
        let gpu = self.gpu;
        let (w, h, _) = levels[0];
        let image = gpu.image(
            &vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_UNORM)
                .extent(vk::Extent3D { width: w, height: h, depth: 1 })
                .mip_levels(levels.len() as u32)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST),
            vk::ImageAspectFlags::COLOR,
        );
        let mut data = Vec::new();
        let mut regions = Vec::new();
        for (l, (lw, lh, texels)) in levels.iter().enumerate() {
            regions.push(
                vk::BufferImageCopy::default()
                    .buffer_offset(data.len() as u64)
                    .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).mip_level(l as u32).layer_count(1))
                    .image_extent(vk::Extent3D { width: *lw, height: *lh, depth: 1 }),
            );
            data.extend_from_slice(bytes_of(texels));
        }
        let staging = gpu.staging(&data);
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(levels.len() as u32).layer_count(1);
        gpu.one_shot(|cb| unsafe {
            let to_dst = vk::ImageMemoryBarrier::default().image(image.image).subresource_range(range).new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL).dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
            gpu.device.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[to_dst]);
            gpu.device.cmd_copy_buffer_to_image(cb, staging.buffer, image.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &regions);
            let to_read = vk::ImageMemoryBarrier::default()
                .image(image.image)
                .subresource_range(range)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);
            gpu.device.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[to_read]);
        });
        gpu.destroy_buffer(staging);
        self.textures.push(Tex { image });
        TextureId(self.textures.len() as u32 - 1)
    }

    fn sampler(&mut self, t: &TextureRef) -> vk::Sampler {
        let key = (t.wrap.0, t.wrap.1, t.linear, t.mipmaps);
        let device = &self.gpu.device;
        *self.samplers.entry(key).or_insert_with(|| {
            let mode = |w| match w {
                Wrap::Clamp => vk::SamplerAddressMode::CLAMP_TO_EDGE,
                Wrap::Repeat => vk::SamplerAddressMode::REPEAT,
                Wrap::Mirror => vk::SamplerAddressMode::MIRRORED_REPEAT,
            };
            let filter = if t.linear { vk::Filter::LINEAR } else { vk::Filter::NEAREST };
            let info = vk::SamplerCreateInfo::default()
                .mag_filter(filter)
                .min_filter(filter)
                .mipmap_mode(if t.mipmaps && t.linear { vk::SamplerMipmapMode::LINEAR } else { vk::SamplerMipmapMode::NEAREST })
                .address_mode_u(mode(t.wrap.0))
                .address_mode_v(mode(t.wrap.1))
                .address_mode_w(vk::SamplerAddressMode::REPEAT)
                .max_lod(if t.mipmaps { 16.0 } else { 0.0 });
            unsafe { device.create_sampler(&info, None) }.unwrap()
        })
    }

    /// The descriptor set binding a material's two layers (made once for each pair).
    fn texture_set(&mut self, layers: [Option<TextureRef>; 2]) -> vk::DescriptorSet {
        if let Some(&s) = self.tex_sets.get(&layers) {
            return s;
        }
        let set = unsafe { self.gpu.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.pool).set_layouts(&[self.tex_layout])) }.expect("descriptor pool full")[0];
        let white = TextureRef { texture: self.white, wrap: (Wrap::Clamp, Wrap::Clamp), linear: false, mipmaps: false };
        for (b, layer) in layers.iter().enumerate() {
            let t = layer.unwrap_or(white);
            let info = [vk::DescriptorImageInfo::default().sampler(self.sampler(&t)).image_view(self.textures[t.texture.0 as usize].image.view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let write = [vk::WriteDescriptorSet::default().dst_set(set).dst_binding(b as u32).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&info)];
            unsafe { self.gpu.device.update_descriptor_sets(&write, &[]) };
        }
        self.tex_sets.insert(layers, set);
        set
    }

    fn pipeline(&mut self, key: PipelineKey) -> vk::Pipeline {
        if let Some(&p) = self.pipelines.get(&key) {
            return p;
        }
        let stages = [
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(self.vert).name(c"main"),
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(self.frag).name(c"main"),
        ];
        let bindings = [vk::VertexInputBindingDescription::default().stride(std::mem::size_of::<Vertex>() as u32).input_rate(vk::VertexInputRate::VERTEX)];
        let attributes = [
            vk::VertexInputAttributeDescription::default().location(0).format(vk::Format::R16G16B16A16_SINT).offset(0),
            vk::VertexInputAttributeDescription::default().location(1).format(vk::Format::R16G16_SINT).offset(8),
            vk::VertexInputAttributeDescription::default().location(2).format(vk::Format::R8G8B8A8_UNORM).offset(12),
            vk::VertexInputAttributeDescription::default().location(3).format(vk::Format::R8G8B8A8_SNORM).offset(16),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&bindings).vertex_attribute_descriptions(&attributes);
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(match key.cull {
                Cull::None => vk::CullModeFlags::NONE,
                Cull::Back => vk::CullModeFlags::BACK,
                Cull::Front => vk::CullModeFlags::FRONT,
            })
            // the projection flips y, so a counter-clockwise triangle on the original's screen
            // is clockwise in Vulkan's
            .front_face(vk::FrontFace::CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default().depth_test_enable(key.depth_test).depth_write_enable(key.depth_write).depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
        let all = vk::ColorComponentFlags::RGBA;
        let colour_blend = match key.blend {
            Blend::Opaque => vk::PipelineColorBlendAttachmentState::default().color_write_mask(all),
            Blend::Alpha => vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(all)
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA),
            Blend::Add => vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(all)
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE)
                .src_alpha_blend_factor(vk::BlendFactor::ZERO)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE),
        };
        let attachments = [colour_blend, vk::PipelineColorBlendAttachmentState::default().color_write_mask(vk::ColorComponentFlags::R)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.pipeline_layout)
            .render_pass(self.render_pass)];
        let p = unsafe { self.gpu.device.create_graphics_pipelines(vk::PipelineCache::null(), &info, None) }.map_err(|e| e.1).unwrap()[0];
        self.pipelines.insert(key, p);
        p
    }

    /// Renders the frame and reads the picture back.
    pub fn render(&mut self, frame: &Frame) -> Picture {
        let gpu = self.gpu;
        if frame.draws.len() > self.draw_capacity {
            let old = std::mem::replace(&mut self.draw_buffer, gpu.buffer(0, vk::BufferUsageFlags::STORAGE_BUFFER, vk::MemoryPropertyFlags::HOST_VISIBLE));
            gpu.destroy_buffer(old);
            self.draw_capacity = frame.draws.len().next_power_of_two();
            self.draw_buffer = gpu.buffer((self.draw_capacity * std::mem::size_of::<DrawRecord>()) as u64, vk::BufferUsageFlags::STORAGE_BUFFER, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT);
            self.bind_draw_buffer();
        }
        let records: Vec<DrawRecord> = frame.draws.iter().map(|d| record(&frame.camera, d)).collect();
        gpu.write(&self.draw_buffer, 0, bytes_of(&records));
        // the pipelines and descriptor sets a draw needs, made before recording
        let prepared: Vec<(vk::Pipeline, vk::DescriptorSet)> = frame
            .draws
            .iter()
            .map(|d| {
                let m = &d.material;
                (self.pipeline(PipelineKey { blend: m.blend, cull: m.cull, depth_test: m.depth_test, depth_write: m.depth_write }), self.texture_set(m.layers))
            })
            .collect();

        let p = frame.camera.projection;
        let push: [f32; 8] = [p[0], p[1], p[2], p[3], p[4], p[5], 0.0, 0.0];
        let d = &gpu.device;
        let cb = unsafe { d.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(gpu.pool).command_buffer_count(1)) }.unwrap()[0];
        let (w, h) = (self.width, self.height);
        unsafe {
            d.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).unwrap();
            let clears = [
                vk::ClearValue { color: vk::ClearColorValue { float32: [0.0; 4] } },
                vk::ClearValue { color: vk::ClearColorValue { uint32: [0; 4] } },
                vk::ClearValue { depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 } },
            ];
            d.cmd_begin_render_pass(
                cb,
                &vk::RenderPassBeginInfo::default().render_pass(self.render_pass).framebuffer(self.framebuffer).render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent: vk::Extent2D { width: w, height: h } }).clear_values(&clears),
                vk::SubpassContents::INLINE,
            );
            d.cmd_set_viewport(cb, 0, &[vk::Viewport { x: 0.0, y: 0.0, width: w as f32, height: h as f32, min_depth: 0.0, max_depth: 1.0 }]);
            d.cmd_set_scissor(cb, 0, &[vk::Rect2D { offset: vk::Offset2D::default(), extent: vk::Extent2D { width: w, height: h } }]);
            d.cmd_push_constants(cb, self.pipeline_layout, vk::ShaderStageFlags::VERTEX, 0, bytes_of(&push));
            d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0, &[self.draw_set], &[]);
            let (mut last_pipeline, mut last_set, mut last_mesh_set) = (vk::Pipeline::null(), vk::DescriptorSet::null(), usize::MAX);
            for (i, draw) in frame.draws.iter().enumerate() {
                let (pipeline, set) = prepared[i];
                if pipeline != last_pipeline {
                    d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, pipeline);
                    last_pipeline = pipeline;
                }
                if set != last_set {
                    d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 1, &[set], &[]);
                    last_set = set;
                }
                let mesh = self.meshes[draw.mesh.0 as usize];
                if mesh.set != last_mesh_set {
                    let set = &self.mesh_sets[mesh.set];
                    d.cmd_bind_vertex_buffers(cb, 0, &[set.vertices.buffer], &[0]);
                    d.cmd_bind_index_buffer(cb, set.indices.buffer, 0, vk::IndexType::UINT32);
                    last_mesh_set = mesh.set;
                }
                d.cmd_draw_indexed(cb, mesh.index_count, 1, mesh.first_index, mesh.vertex_offset, i as u32);
            }
            d.cmd_end_render_pass(cb);
            let region = |_: ()| vk::BufferImageCopy::default().image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1)).image_extent(vk::Extent3D { width: w, height: h, depth: 1 });
            d.cmd_copy_image_to_buffer(cb, self.colour.image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, self.colour_out.buffer, &[region(())]);
            d.cmd_copy_image_to_buffer(cb, self.ids.image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, self.ids_out.buffer, &[region(())]);
            d.end_command_buffer(cb).unwrap();
            let fence = d.create_fence(&vk::FenceCreateInfo::default(), None).unwrap();
            d.queue_submit(gpu.queue, &[vk::SubmitInfo::default().command_buffers(&[cb])], fence).unwrap();
            assert!(d.wait_for_fences(&[fence], true, 30_000_000_000).is_ok(), "the frame did not finish within 30 s");
            d.destroy_fence(fence, None);
            d.free_command_buffers(gpu.pool, &[cb]);
        }
        let n = (w * h) as usize;
        let rgba = gpu.read(&self.colour_out, n * 4);
        let raw = gpu.read(&self.ids_out, n * 4);
        let ids = raw.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).map(|i| if i == 0 { 0 } else { frame.draws[i as usize - 1].id }).collect();
        Picture { width: w, height: h, rgba, ids }
    }
}

impl Drop for Renderer<'_> {
    fn drop(&mut self) {
        let gpu = self.gpu;
        let d = &gpu.device;
        unsafe {
            d.device_wait_idle().ok();
            for (_, p) in self.pipelines.drain() {
                d.destroy_pipeline(p, None);
            }
            for (_, s) in self.samplers.drain() {
                d.destroy_sampler(s, None);
            }
            d.destroy_descriptor_pool(self.pool, None);
            d.destroy_pipeline_layout(self.pipeline_layout, None);
            d.destroy_descriptor_set_layout(self.draw_layout, None);
            d.destroy_descriptor_set_layout(self.tex_layout, None);
            d.destroy_shader_module(self.vert, None);
            d.destroy_shader_module(self.frag, None);
            d.destroy_framebuffer(self.framebuffer, None);
            d.destroy_render_pass(self.render_pass, None);
        }
        for t in self.textures.drain(..) {
            gpu.destroy_image(t.image);
        }
        for s in self.mesh_sets.drain(..) {
            gpu.destroy_buffer(s.vertices);
            gpu.destroy_buffer(s.indices);
        }
        // the images and buffers made in `new` are plain handles: take them out by value
        let (colour, ids, depth) = (std::mem::replace(&mut self.colour, empty_image()), std::mem::replace(&mut self.ids, empty_image()), std::mem::replace(&mut self.depth, empty_image()));
        gpu.destroy_image(colour);
        gpu.destroy_image(ids);
        gpu.destroy_image(depth);
        for b in [&self.colour_out, &self.ids_out, &self.draw_buffer] {
            unsafe {
                d.destroy_buffer(b.buffer, None);
                d.free_memory(b.memory, None);
            }
        }
    }
}

fn empty_image() -> Image {
    Image { image: vk::Image::null(), memory: vk::DeviceMemory::null(), view: vk::ImageView::null() }
}

fn attachment(format: vk::Format, final_layout: vk::ImageLayout) -> vk::AttachmentDescription {
    vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(final_layout)
}

/// The inverse transpose of a 3x3 (rows), which is what normals are carried by.
fn normal_matrix(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let c = |a: usize, b: usize, p: usize, q: usize| m[a][p] * m[b][q] - m[a][q] * m[b][p];
    // cofactors
    let cof = [
        [c(1, 2, 1, 2), -c(1, 2, 0, 2), c(1, 2, 0, 1)],
        [-c(0, 2, 1, 2), c(0, 2, 0, 2), -c(0, 2, 0, 1)],
        [c(0, 1, 1, 2), -c(0, 1, 0, 2), c(0, 1, 0, 1)],
    ];
    let det = m[0][0] * cof[0][0] + m[0][1] * cof[0][1] + m[0][2] * cof[0][2];
    let s = if det != 0.0 { 1.0 / det } else { 1.0 };
    cof.map(|r| r.map(|v| v * s))
}

fn record(camera: &Camera, d: &Draw) -> DrawRecord {
    let view = d.view.unwrap_or(camera.view);
    let mv = concat(&view, &d.world);
    let mut r = DrawRecord::default();
    for i in 0..3 {
        r.mv[i] = [mv[i * 4], mv[i * 4 + 1], mv[i * 4 + 2], mv[i * 4 + 3]];
    }
    let nm = normal_matrix([[mv[0], mv[1], mv[2]], [mv[4], mv[5], mv[6]], [mv[8], mv[9], mv[10]]]);
    for i in 0..3 {
        r.nm[i] = [nm[i][0], nm[i][1], nm[i][2], 0.0];
    }
    let (mode, combine) = (
        match &d.lighting {
            Lighting::Lit { .. } => 0.0,
            Lighting::VertexColours => 1.0,
            Lighting::Black => 2.0,
        },
        match d.material.combine {
            Combine::Vertex => 0,
            Combine::Modulate { unlit: false } => 1,
            Combine::Modulate { unlit: true } => 2,
            Combine::Crossfade { .. } => 3,
            Combine::Over { by_texture_alpha: false, .. } => 4,
            Combine::Over { by_texture_alpha: true, .. } => 5,
            Combine::Add { .. } => 6,
        },
    );
    r.ambient[3] = mode;
    if let Lighting::Lit { ambient, directionals, points } = &d.lighting {
        r.ambient[..3].copy_from_slice(ambient);
        for (i, l) in directionals.iter().take(MAX_DIRECTIONALS).enumerate() {
            r.dir_colour[i] = [l.colour[0], l.colour[1], l.colour[2], 0.0];
            r.dir_to_light[i] = [l.to_light[0], l.to_light[1], l.to_light[2], 0.0];
        }
        for (i, l) in points.iter().take(MAX_POINTS).enumerate() {
            r.point_colour[i] = [l.colour[0], l.colour[1], l.colour[2], l.k0];
            r.point_position[i] = [l.position[0], l.position[1], l.position[2], l.k1];
        }
        r.counts[0] = directionals.len().min(MAX_DIRECTIONALS) as i32;
        r.counts[1] = points.len().min(MAX_POINTS) as i32;
    }
    let unlit_first = matches!(d.material.combine, Combine::Over { unlit_first: true, .. } | Combine::Add { unlit_first: true });
    r.counts[2] = combine;
    r.counts[3] = i32::from(d.material.alpha_ref.is_some()) | if unlit_first { 2 } else { 0 };
    r.params[0] = if let Combine::Crossfade { fade } = d.material.combine { fade } else { 0.0 };
    r.params[1] = d.material.alpha_ref.unwrap_or(0.0);
    r
}
