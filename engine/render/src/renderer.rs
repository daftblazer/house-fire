//! The renderer: meshes and textures uploaded once, then a frame's draws recorded into one
//! command buffer and rendered offscreen. Per-draw numbers (matrices, light set, material
//! switches) are one storage buffer indexed by the instance; no vertex is touched on the CPU.

use crate::frame::*;
use crate::gpu::{Buffer, Gpu, Image};
use ash::vk;
use std::collections::HashMap;

const COLOUR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const ID_FORMAT: vk::Format = vk::Format::R32G32_UINT;
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
    /// x: the mesh's texture coordinate unit (`MeshData::uv_shift`).
    extra: [f32; 4],
    /// Layer 1's position-made coordinates (`extra.y` 1 when on): u then v, each (x, y, offset).
    layer1: [[f32; 4]; 2],
    /// The projection (`Camera::projection`, or the draw's own): the first four values, then z's two and padding.
    proj: [[f32; 4]; 2],
    /// x: added to a vertex's index to find it among the skinned vertices (`skin.comp`'s output).
    skin: [i32; 4],
}

/// A skinned draw's job for the compute pass (`shaders/skin.comp`'s push constants).
#[derive(Clone, Copy)]
#[repr(C)]
struct SkinJob {
    first: u32,
    count: u32,
    bones: u32,
    base: u32,
}

/// A vertex the compute pass leaves: the blended position and the turned normal (`skin.comp`).
const SKINNED_VERTEX_BYTES: usize = 32;

/// The compute pass's work group size (`skin.comp`'s local_size_x).
const SKIN_GROUP: u32 = 64;

/// One skinned draw's blended vertices, read back from the GPU (`Renderer::read_skinned`).
pub struct SkinnedDraw {
    /// The draw's place in the frame.
    pub draw: usize,
    pub positions: Vec<[f32; 3]>,
    /// The normals turned by the heaviest bone, unit length.
    pub normals: Vec<[f32; 3]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    blend: Blend,
    cull: Cull,
    depth_test: Option<Compare>,
    depth_write: bool,
    write_colour: bool,
    write_alpha: bool,
    dst_alpha: Option<u8>,
    /// The vertices come from the compute pass (a skinned mesh's draw).
    skinned: bool,
}

struct MeshSet {
    vertices: Buffer,
    indices: Buffer,
    /// The weights of a set with skinned meshes (two words a vertex), and the compute pass's
    /// descriptor set over them.
    skin: Option<(Buffer, vk::DescriptorSet)>,
}

#[derive(Clone, Copy)]
struct MeshSlot {
    set: usize,
    first_index: u32,
    index_count: u32,
    vertex_offset: i32,
    vertex_count: u32,
    skinned: bool,
    uv_unit: f32,
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
    vert_skinned: vk::ShaderModule,
    frag: vk::ShaderModule,
    skin_layout: vk::DescriptorSetLayout,
    skin_pipeline_layout: vk::PipelineLayout,
    skin_pipeline: vk::Pipeline,
    skin_module: vk::ShaderModule,
    /// Each skinned draw's four bones (twelve vec4s), and the vertices they blend into.
    bones_buffer: Buffer,
    skinned_buffer: Buffer,
    bones_capacity: usize,
    skinned_capacity: usize,
    /// Where the last frame's skinned draws went in `skinned_buffer`: (draw, first vertex, count).
    last_skinned: Vec<(usize, u32, u32)>,
    pipelines: HashMap<PipelineKey, vk::Pipeline>,
    samplers: HashMap<(Wrap, Wrap, bool, Mipmaps), vk::Sampler>,
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
        let ids_out = gpu.buffer(u64::from(width * height * 8), vk::BufferUsageFlags::TRANSFER_DST, host);

        let draw_layout = unsafe {
            d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                    vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
                    vk::DescriptorSetLayoutBinding::default().binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::VERTEX),
                ]),
                None,
            )
        }
        .unwrap();
        let tex_bindings = [0, 1].map(|b| vk::DescriptorSetLayoutBinding::default().binding(b).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT));
        let tex_layout = unsafe { d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&tex_bindings), None) }.unwrap();
        let layouts = [draw_layout, tex_layout];
        let pipeline_layout = unsafe { d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts), None) }.unwrap();
        // the skinning pass: a mesh set's vertices and weights, the bones, the output
        let skin_bindings = [0, 1, 2, 3].map(|b| vk::DescriptorSetLayoutBinding::default().binding(b).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE));
        let skin_layout = unsafe { d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&skin_bindings), None) }.unwrap();
        let skin_layouts = [skin_layout];
        let skin_push = [vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::COMPUTE).size(std::mem::size_of::<SkinJob>() as u32)];
        let skin_pipeline_layout = unsafe { d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&skin_layouts).push_constant_ranges(&skin_push), None) }.unwrap();
        let module = |spv: &[u8]| {
            let words: Vec<u32> = spv.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
            unsafe { d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None) }.unwrap()
        };
        let vert = module(include_bytes!(concat!(env!("OUT_DIR"), "/model.vert.spv")));
        let vert_skinned = module(include_bytes!(concat!(env!("OUT_DIR"), "/model_skinned.vert.spv")));
        let frag = module(include_bytes!(concat!(env!("OUT_DIR"), "/model.frag.spv")));
        let skin_module = module(include_bytes!(concat!(env!("OUT_DIR"), "/skin.comp.spv")));
        let skin_pipeline = unsafe {
            d.create_compute_pipelines(
                vk::PipelineCache::null(),
                &[vk::ComputePipelineCreateInfo::default().layout(skin_pipeline_layout).stage(vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::COMPUTE).module(skin_module).name(c"main"))],
                None,
            )
        }
        .map_err(|e| e.1)
        .unwrap()[0];
        let pool_sizes = [
            vk::DescriptorPoolSize::default().ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(8192),
            vk::DescriptorPoolSize::default().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(256),
        ];
        let pool = unsafe { d.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(4100).pool_sizes(&pool_sizes), None) }.unwrap();
        let draw_set = unsafe { d.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&[draw_layout])) }.unwrap()[0];
        let draw_capacity = 4096;
        let draw_buffer = gpu.buffer((draw_capacity * std::mem::size_of::<DrawRecord>()) as u64, vk::BufferUsageFlags::STORAGE_BUFFER, host);
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let (bones_capacity, skinned_capacity) = (64, 8192);
        let bones_buffer = gpu.buffer((bones_capacity * 192) as u64, storage, host);
        let skinned_buffer = gpu.buffer((skinned_capacity * SKINNED_VERTEX_BYTES) as u64, storage, host);
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
            vert_skinned,
            frag,
            skin_layout,
            skin_pipeline_layout,
            skin_pipeline,
            skin_module,
            bones_buffer,
            skinned_buffer,
            bones_capacity,
            skinned_capacity,
            last_skinned: Vec::new(),
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
        let draws = [vk::DescriptorBufferInfo::default().buffer(self.draw_buffer.buffer).range(vk::WHOLE_SIZE)];
        let skinned = [vk::DescriptorBufferInfo::default().buffer(self.skinned_buffer.buffer).range(vk::WHOLE_SIZE)];
        let write = [
            vk::WriteDescriptorSet::default().dst_set(self.draw_set).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&draws),
            vk::WriteDescriptorSet::default().dst_set(self.draw_set).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&skinned),
        ];
        unsafe { self.gpu.device.update_descriptor_sets(&write, &[]) };
        // the compute pass reads the bones and writes the same skinned vertices
        for set in self.mesh_sets.iter().filter_map(|m| m.skin.as_ref()) {
            let bones = [vk::DescriptorBufferInfo::default().buffer(self.bones_buffer.buffer).range(vk::WHOLE_SIZE)];
            let write = [
                vk::WriteDescriptorSet::default().dst_set(set.1).dst_binding(2).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&bones),
                vk::WriteDescriptorSet::default().dst_set(set.1).dst_binding(3).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&skinned),
            ];
            unsafe { self.gpu.device.update_descriptor_sets(&write, &[]) };
        }
    }

    /// Uploads meshes into one vertex and one index buffer, once; the ids come back in order.
    pub fn load_meshes(&mut self, meshes: &[MeshData]) -> Vec<MeshId> {
        let set = self.mesh_sets.len();
        let (mut vertices, mut indices) = (Vec::new(), Vec::<u32>::new());
        let mut weights: Vec<[u32; 2]> = Vec::new();
        let mut ids = Vec::new();
        for m in meshes {
            ids.push(MeshId(self.meshes.len() as u32));
            let skinned = !m.skin.is_empty();
            assert!(!skinned || m.skin.len() == m.vertices.len(), "a skinned mesh has a blend for every vertex");
            self.meshes.push(MeshSlot { set, first_index: indices.len() as u32, index_count: m.indices.len() as u32, vertex_offset: vertices.len() as i32, vertex_count: m.vertices.len() as u32, skinned, uv_unit: f32::from(1u16 << m.uv_shift) });
            vertices.extend_from_slice(&m.vertices);
            indices.extend_from_slice(&m.indices);
            // two words a vertex: the weight bytes, the normal's bone (zeros for a rigid mesh)
            if skinned || !weights.is_empty() {
                weights.resize(vertices.len() - m.vertices.len(), [0, 0]);
                weights.extend(m.skin.iter().map(|w| [u32::from_le_bytes(w.weights), u32::from(w.heaviest)]));
                weights.resize(vertices.len(), [0, 0]);
            }
        }
        let vertex_buffer = self.gpu.upload(bytes_of(&vertices), vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::STORAGE_BUFFER);
        let skin = (!weights.is_empty()).then(|| {
            let weight_buffer = self.gpu.upload(bytes_of(&weights), vk::BufferUsageFlags::STORAGE_BUFFER);
            let layouts = [self.skin_layout];
            let descriptor = unsafe { self.gpu.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.pool).set_layouts(&layouts)) }.unwrap()[0];
            let vertex_info = [vk::DescriptorBufferInfo::default().buffer(vertex_buffer.buffer).range(vk::WHOLE_SIZE)];
            let weight_info = [vk::DescriptorBufferInfo::default().buffer(weight_buffer.buffer).range(vk::WHOLE_SIZE)];
            let write = [
                vk::WriteDescriptorSet::default().dst_set(descriptor).dst_binding(0).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&vertex_info),
                vk::WriteDescriptorSet::default().dst_set(descriptor).dst_binding(1).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(&weight_info),
            ];
            unsafe { self.gpu.device.update_descriptor_sets(&write, &[]) };
            (weight_buffer, descriptor)
        });
        self.mesh_sets.push(MeshSet { vertices: vertex_buffer, indices: self.gpu.upload(bytes_of(&indices), vk::BufferUsageFlags::INDEX_BUFFER), skin });
        self.bind_draw_buffer();
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
                .mipmap_mode(if t.mipmaps == Mipmaps::Linear { vk::SamplerMipmapMode::LINEAR } else { vk::SamplerMipmapMode::NEAREST })
                .address_mode_u(mode(t.wrap.0))
                .address_mode_v(mode(t.wrap.1))
                .address_mode_w(vk::SamplerAddressMode::REPEAT)
                .max_lod(if t.mipmaps == Mipmaps::Off { 0.0 } else { 16.0 });
            unsafe { device.create_sampler(&info, None) }.unwrap()
        })
    }

    /// The descriptor set binding a material's two layers (made once for each pair).
    fn texture_set(&mut self, layers: [Option<TextureRef>; 2]) -> vk::DescriptorSet {
        if let Some(&s) = self.tex_sets.get(&layers) {
            return s;
        }
        let set = unsafe { self.gpu.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.pool).set_layouts(&[self.tex_layout])) }.expect("descriptor pool full")[0];
        let white = TextureRef { texture: self.white, wrap: (Wrap::Clamp, Wrap::Clamp), linear: false, mipmaps: Mipmaps::Off };
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
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(if key.skinned { self.vert_skinned } else { self.vert }).name(c"main"),
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
                Cull::All => vk::CullModeFlags::FRONT_AND_BACK,
            })
            // the projection flips y, so a counter-clockwise triangle on the original's screen
            // is clockwise in Vulkan's
            .front_face(vk::FrontFace::CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default().depth_test_enable(key.depth_test.is_some())
            .depth_write_enable(key.depth_write)
            .depth_compare_op(key.depth_test.map_or(vk::CompareOp::ALWAYS, |c| vk::CompareOp::from_raw(i32::from(c.0))));
        let mut all = vk::ColorComponentFlags::empty();
        if key.write_colour {
            all |= vk::ColorComponentFlags::R | vk::ColorComponentFlags::G | vk::ColorComponentFlags::B;
        }
        if key.write_alpha {
            all |= vk::ColorComponentFlags::A;
        }
        let factor = |f: Factor, src: bool| match f {
            Factor::Zero => vk::BlendFactor::ZERO,
            Factor::One => vk::BlendFactor::ONE,
            Factor::Other if src => vk::BlendFactor::DST_COLOR,
            Factor::Other => vk::BlendFactor::SRC_COLOR,
            Factor::InvOther if src => vk::BlendFactor::ONE_MINUS_DST_COLOR,
            Factor::InvOther => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
            Factor::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
            Factor::InvSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            Factor::DstAlpha => vk::BlendFactor::DST_ALPHA,
            Factor::InvDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
        };
        let colour_blend = match key.blend {
            Blend::Opaque => vk::PipelineColorBlendAttachmentState::default().color_write_mask(all),
            // GX blends the colour and the alpha by the same factors
            Blend::Mix { src, dst } => vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(all)
                .blend_enable(true)
                .src_color_blend_factor(factor(src, true))
                .dst_color_blend_factor(factor(dst, false))
                .src_alpha_blend_factor(factor(src, true))
                .dst_alpha_blend_factor(factor(dst, false)),
            Blend::Subtract => vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(all)
                .blend_enable(true)
                .color_blend_op(vk::BlendOp::REVERSE_SUBTRACT)
                .alpha_blend_op(vk::BlendOp::REVERSE_SUBTRACT)
                .src_color_blend_factor(vk::BlendFactor::ONE)
                .dst_color_blend_factor(vk::BlendFactor::ONE)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE),
        };
        // a constant destination alpha replaces the alpha the blend makes: the shader writes
        // the constant as the fragment's alpha (`record`), taken as it is
        let colour_blend = match key.dst_alpha {
            Some(_) => colour_blend
                .blend_enable(true)
                .src_color_blend_factor(if key.blend == Blend::Opaque { vk::BlendFactor::ONE } else { colour_blend.src_color_blend_factor })
                .dst_color_blend_factor(if key.blend == Blend::Opaque { vk::BlendFactor::ZERO } else { colour_blend.dst_color_blend_factor })
                .alpha_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ZERO),
            None => colour_blend,
        };
        // the id picture: the depth-tested draws write the first channel, the shadows' passes the second
        let id_mask = match (key.write_colour, key.depth_test.is_some()) {
            (false, _) => vk::ColorComponentFlags::empty(),
            (true, true) => vk::ColorComponentFlags::R,
            (true, false) => vk::ColorComponentFlags::G,
        };
        let attachments = [colour_blend, vk::PipelineColorBlendAttachmentState::default().color_write_mask(id_mask)];
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
        // the skinned draws: each gets its own run of blended vertices and four bones
        let mut jobs: Vec<(usize, usize, SkinJob)> = Vec::new();
        let mut bone_rows: Vec<[f32; 4]> = Vec::new();
        let mut skinned_vertices = 0u32;
        let mut skin_delta = vec![0i32; frame.draws.len()];
        for (i, d) in frame.draws.iter().enumerate() {
            let mesh = self.meshes[d.mesh.0 as usize];
            let Some(bones) = d.bones.as_ref().filter(|_| mesh.skinned) else { continue };
            jobs.push((mesh.set, i, SkinJob { first: mesh.vertex_offset as u32, count: mesh.vertex_count, bones: bone_rows.len() as u32, base: skinned_vertices }));
            for m in bones.iter() {
                bone_rows.extend(m.chunks(4).map(|r| [r[0], r[1], r[2], r[3]]));
            }
            // the vertex shader finds a vertex by its index plus the mesh's offset in the set
            skin_delta[i] = skinned_vertices as i32 - mesh.vertex_offset;
            skinned_vertices += mesh.vertex_count;
        }
        if bone_rows.len() > self.bones_capacity * 12 || skinned_vertices as usize > self.skinned_capacity {
            let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
            self.bones_capacity = self.bones_capacity.max((bone_rows.len() / 12).next_power_of_two());
            self.skinned_capacity = self.skinned_capacity.max((skinned_vertices as usize).next_power_of_two());
            let old = (std::mem::replace(&mut self.bones_buffer, gpu.buffer((self.bones_capacity * 192) as u64, vk::BufferUsageFlags::STORAGE_BUFFER, host)), std::mem::replace(&mut self.skinned_buffer, gpu.buffer((self.skinned_capacity * SKINNED_VERTEX_BYTES) as u64, vk::BufferUsageFlags::STORAGE_BUFFER, host)));
            unsafe { gpu.device.device_wait_idle().ok() };
            gpu.destroy_buffer(old.0);
            gpu.destroy_buffer(old.1);
            self.bind_draw_buffer();
        }
        gpu.write(&self.bones_buffer, 0, bytes_of(&bone_rows));
        self.last_skinned = jobs.iter().map(|&(_, i, j)| (i, j.base, j.count)).collect();
        let records: Vec<DrawRecord> = frame.draws.iter().enumerate().map(|(i, d)| record(&frame.camera, d, self.meshes[d.mesh.0 as usize].uv_unit, skin_delta[i])).collect();
        gpu.write(&self.draw_buffer, 0, bytes_of(&records));
        // the pipelines and descriptor sets a draw needs, made before recording
        let prepared: Vec<(vk::Pipeline, vk::DescriptorSet)> = frame
            .draws
            .iter()
            .map(|d| {
                let m = &d.material;
                (self.pipeline(PipelineKey { blend: m.blend, cull: m.cull, depth_test: m.depth_test, depth_write: m.depth_write, write_colour: m.write_colour, write_alpha: m.write_alpha, dst_alpha: m.dst_alpha, skinned: d.bones.is_some() && self.meshes[d.mesh.0 as usize].skinned }), self.texture_set(m.layers))
            })
            .collect();

        let d = &gpu.device;
        let cb = unsafe { d.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(gpu.pool).command_buffer_count(1)) }.unwrap()[0];
        let (w, h) = (self.width, self.height);
        unsafe {
            d.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).unwrap();
            if !jobs.is_empty() {
                // the skinning pass: every skinned draw's vertices, before the render pass reads them
                d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.skin_pipeline);
                for (set, _, job) in &jobs {
                    let descriptor = self.mesh_sets[*set].skin.as_ref().expect("a skinned mesh's set has weights").1;
                    d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::COMPUTE, self.skin_pipeline_layout, 0, &[descriptor], &[]);
                    d.cmd_push_constants(cb, self.skin_pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, bytes_of(std::slice::from_ref(job)));
                    d.cmd_dispatch(cb, job.count.div_ceil(SKIN_GROUP), 1, 1);
                }
                let barrier = [vk::MemoryBarrier::default().src_access_mask(vk::AccessFlags::SHADER_WRITE).dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::HOST_READ)];
                d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::COMPUTE_SHADER, vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::HOST, vk::DependencyFlags::empty(), &barrier, &[], &[]);
            }
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
            d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0, &[self.draw_set], &[]);
            let (mut last_pipeline, mut last_set, mut last_mesh_set) = (vk::Pipeline::null(), vk::DescriptorSet::null(), usize::MAX);
            let mut last_viewport: Option<Viewport> = None;
            for (i, draw) in frame.draws.iter().enumerate() {
                if draw.viewport != last_viewport {
                    // an inner viewport (the interface's portrait) draws on a cleared depth buffer
                    let v = draw.viewport.unwrap_or(Viewport { x: 0.0, y: 0.0, width: w as f32, height: h as f32 });
                    let (px, py, pw, ph) = v.pixels();
                    let rect = vk::Rect2D { offset: vk::Offset2D { x: px, y: py }, extent: vk::Extent2D { width: pw, height: ph } };
                    d.cmd_set_viewport(cb, 0, &[vk::Viewport { x: v.x, y: v.y, width: v.width, height: v.height, min_depth: 0.0, max_depth: 1.0 }]);
                    d.cmd_set_scissor(cb, 0, &[rect]);
                    if draw.viewport.is_some() {
                        let clear = [vk::ClearAttachment::default().aspect_mask(vk::ImageAspectFlags::DEPTH).clear_value(vk::ClearValue { depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 } })];
                        d.cmd_clear_attachments(cb, &clear, &[vk::ClearRect { rect, base_array_layer: 0, layer_count: 1 }]);
                        if let Some((b, rgb)) = frame.backdrop {
                            let (bx, by, bw, bh) = b.pixels();
                            let rect = vk::Rect2D { offset: vk::Offset2D { x: bx, y: by }, extent: vk::Extent2D { width: bw, height: bh } };
                            let colour = [vk::ClearAttachment::default().aspect_mask(vk::ImageAspectFlags::COLOR).color_attachment(0).clear_value(vk::ClearValue { color: vk::ClearColorValue { float32: [f32::from(rgb[0]) / 255.0, f32::from(rgb[1]) / 255.0, f32::from(rgb[2]) / 255.0, 1.0] } })];
                            d.cmd_clear_attachments(cb, &colour, &[vk::ClearRect { rect, base_array_layer: 0, layer_count: 1 }]);
                        }
                    }
                    last_viewport = draw.viewport;
                }
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
        let raw = gpu.read(&self.ids_out, n * 8);
        let id_of = |c: &[u8]| match u32::from_le_bytes(c.try_into().unwrap()) {
            0 => 0,
            i => frame.draws[i as usize - 1].id,
        };
        let ids = raw.as_chunks::<8>().0.iter().map(|c| id_of(&c[..4])).collect();
        let late_ids = raw.as_chunks::<8>().0.iter().map(|c| id_of(&c[4..])).collect();
        Picture { width: w, height: h, rgba, ids, late_ids }
    }
}

impl Renderer<'_> {
    /// The blended vertices of the last frame's skinned draws, read back from the GPU (for
    /// checking them against the reference skinner).
    pub fn read_skinned(&self) -> Vec<SkinnedDraw> {
        let total = self.last_skinned.iter().map(|&(_, b, c)| (b + c) as usize).max().unwrap_or(0);
        let raw = self.gpu.read(&self.skinned_buffer, total * SKINNED_VERTEX_BYTES);
        let floats: Vec<f32> = raw.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        self.last_skinned
            .iter()
            .map(|&(draw, base, count)| {
                let vertex = |v: u32, at: usize| {
                    let o = (base + v) as usize * 8 + at;
                    [floats[o], floats[o + 1], floats[o + 2]]
                };
                SkinnedDraw { draw, positions: (0..count).map(|v| vertex(v, 0)).collect(), normals: (0..count).map(|v| vertex(v, 4)).collect() }
            })
            .collect()
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
            d.destroy_pipeline(self.skin_pipeline, None);
            d.destroy_pipeline_layout(self.skin_pipeline_layout, None);
            d.destroy_descriptor_set_layout(self.skin_layout, None);
            d.destroy_shader_module(self.skin_module, None);
            d.destroy_shader_module(self.vert_skinned, None);
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
            if let Some((weights, _)) = s.skin {
                gpu.destroy_buffer(weights);
            }
        }
        // the images and buffers made in `new` are plain handles: take them out by value
        let (colour, ids, depth) = (std::mem::replace(&mut self.colour, empty_image()), std::mem::replace(&mut self.ids, empty_image()), std::mem::replace(&mut self.depth, empty_image()));
        gpu.destroy_image(colour);
        gpu.destroy_image(ids);
        gpu.destroy_image(depth);
        for b in [&self.colour_out, &self.ids_out, &self.draw_buffer, &self.bones_buffer, &self.skinned_buffer] {
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

fn record(camera: &Camera, d: &Draw, uv_unit: f32, skin_delta: i32) -> DrawRecord {
    let view = d.view.unwrap_or(camera.view);
    let mv = concat(&view, &d.world);
    let mut r = DrawRecord::default();
    let p = d.projection.unwrap_or(camera.projection);
    r.proj = [[p[0], p[1], p[2], p[3]], [p[4], p[5], 0.0, 0.0]];
    r.skin[0] = skin_delta;
    for i in 0..3 {
        r.mv[i] = [mv[i * 4], mv[i * 4 + 1], mv[i * 4 + 2], mv[i * 4 + 3]];
    }
    let nm = normal_matrix([[mv[0], mv[1], mv[2]], [mv[4], mv[5], mv[6]], [mv[8], mv[9], mv[10]]]);
    for (row, n) in r.nm.iter_mut().zip(nm) {
        *row = [n[0], n[1], n[2], 0.0];
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
    r.extra[0] = uv_unit;
    // the constant alpha goes out as the fragment's alpha, except where the colour blend reads
    // the fragment's alpha itself (then the alpha written is the fragment's own)
    let reads_source_alpha = matches!(d.material.blend, Blend::Mix { src, dst } if [src, dst].iter().any(|f| matches!(f, Factor::SrcAlpha | Factor::InvSrcAlpha)));
    if let Some(a) = d.material.dst_alpha.filter(|_| !reads_source_alpha) {
        r.extra[2] = 1.0;
        r.extra[3] = f32::from(a) / 255.0;
    }
    if let Some(g) = d.material.layer1_from_position {
        r.extra[1] = 1.0;
        r.layer1 = [[g.u[0], g.u[1], g.u[2], 0.0], [g.v[0], g.v[1], g.v[2], 0.0]];
    }
    r.counts[2] = combine;
    // flags: 1 alpha test (its functions in bits 5 to 12), 2 first layer unlit, 4 sphere map added (weight in bits 3 and 4)
    let (sphere, weight) = match d.material.sphere {
        None => (0, 0),
        Some(SphereWeight::One) => (4, 0),
        Some(SphereWeight::PrevAlpha) => (4, 1 << 3),
        Some(SphereWeight::Factor(f)) => {
            r.params[2] = f;
            (4, 2 << 3)
        }
    };
    // alpha test: comp0 in bits 5-7, comp1 in 8-10, op in 11-12; the references in params.y, w
    let alpha = d.material.alpha_test.map_or(0, |t| 1 | i32::from(t.comp[0].0) << 5 | i32::from(t.comp[1].0) << 8 | i32::from(t.op) << 11);
    r.counts[3] = alpha | if unlit_first { 2 } else { 0 } | sphere | weight;
    r.params[0] = if let Combine::Crossfade { fade } = d.material.combine { fade } else { 0.0 };
    if let Some(t) = d.material.alpha_test {
        (r.params[1], r.params[3]) = (f32::from(t.refs[0]), f32::from(t.refs[1]));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_matrix_keeps_directions_through_scale_and_mirror() {
        // a mirror in y with a uniform scale: normals are mirrored too, and not scaled up
        let n = normal_matrix([[0.5, 0.0, 0.0], [0.0, -0.5, 0.0], [0.0, 0.0, 0.5]]);
        assert_eq!(n[1][1], -2.0);
        assert_eq!(n[0][0], 2.0);
        assert_eq!(n[0][1], 0.0);
    }
}
