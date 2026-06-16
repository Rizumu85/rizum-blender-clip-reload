use std::cell::OnceCell;

use clip_model::CanvasSize;

use crate::shaders::*;
use crate::stream_bounds::CanvasRect;
use crate::types::GpuNormalRasterSource;
use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuRasterBlendMode, GpuRasterResourceCache,
    GpuRenderError,
};
pub(crate) struct NormalStackPipelines {
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    alpha_pipeline: OnceCell<wgpu::RenderPipeline>,
    clipped_pipeline: OnceCell<wgpu::RenderPipeline>,
    clipped_byte_pipeline: OnceCell<wgpu::RenderPipeline>,
    through_pipeline: OnceCell<wgpu::RenderPipeline>,
    add_glow_pipeline: OnceCell<wgpu::RenderPipeline>,
    color_dodge_pipeline: OnceCell<wgpu::RenderPipeline>,
    color_burn_pipeline: OnceCell<wgpu::RenderPipeline>,
    glow_dodge_pipeline: OnceCell<wgpu::RenderPipeline>,
    standard_blend_pipeline: OnceCell<wgpu::RenderPipeline>,
    lut_filter_pipeline: OnceCell<wgpu::RenderPipeline>,
}

impl NormalStackPipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = create_normal_source_bind_group_layout(device);
        Self {
            bind_group_layout,
            alpha_pipeline: OnceCell::new(),
            clipped_pipeline: OnceCell::new(),
            clipped_byte_pipeline: OnceCell::new(),
            through_pipeline: OnceCell::new(),
            add_glow_pipeline: OnceCell::new(),
            color_dodge_pipeline: OnceCell::new(),
            color_burn_pipeline: OnceCell::new(),
            glow_dodge_pipeline: OnceCell::new(),
            standard_blend_pipeline: OnceCell::new(),
            lut_filter_pipeline: OnceCell::new(),
        }
    }

    pub(crate) fn alpha_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.alpha_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                NORMAL_ALPHA_OVER_SHADER,
                "rizum_clip_normal_alpha_shader",
                "rizum_clip_normal_alpha_pipeline",
                "rizum_clip_normal_alpha_pipeline_layout",
            )
        })
    }

    pub(crate) fn clipped_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.clipped_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                CLIPPED_NORMAL_PRESERVE_SHADER,
                "rizum_clip_clipped_normal_preserve_shader",
                "rizum_clip_clipped_normal_preserve_pipeline",
                "rizum_clip_clipped_normal_preserve_pipeline_layout",
            )
        })
    }

    pub(crate) fn clipped_byte_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.clipped_byte_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                CLIPPED_BYTE_PRESERVE_SHADER,
                "rizum_clip_clipped_byte_preserve_shader",
                "rizum_clip_clipped_byte_preserve_pipeline",
                "rizum_clip_clipped_byte_preserve_pipeline_layout",
            )
        })
    }

    pub(crate) fn through_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.through_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                THROUGH_GROUP_RESOLVE_SHADER,
                "rizum_clip_through_group_resolve_shader",
                "rizum_clip_through_group_resolve_pipeline",
                "rizum_clip_through_group_resolve_pipeline_layout",
            )
        })
    }

    pub(crate) fn add_glow_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.add_glow_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                ADD_GLOW_SHADER,
                "rizum_clip_add_glow_shader",
                "rizum_clip_add_glow_pipeline",
                "rizum_clip_add_glow_pipeline_layout",
            )
        })
    }

    pub(crate) fn color_dodge_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.color_dodge_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                COLOR_DODGE_SHADER,
                "rizum_clip_color_dodge_shader",
                "rizum_clip_color_dodge_pipeline",
                "rizum_clip_color_dodge_pipeline_layout",
            )
        })
    }

    pub(crate) fn color_burn_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.color_burn_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                COLOR_BURN_SHADER,
                "rizum_clip_color_burn_shader",
                "rizum_clip_color_burn_pipeline",
                "rizum_clip_color_burn_pipeline_layout",
            )
        })
    }

    pub(crate) fn glow_dodge_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.glow_dodge_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                GLOW_DODGE_SHADER,
                "rizum_clip_glow_dodge_shader",
                "rizum_clip_glow_dodge_pipeline",
                "rizum_clip_glow_dodge_pipeline_layout",
            )
        })
    }

    pub(crate) fn standard_blend_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.standard_blend_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                STANDARD_BLEND_SHADER,
                "rizum_clip_standard_blend_shader",
                "rizum_clip_standard_blend_pipeline",
                "rizum_clip_standard_blend_pipeline_layout",
            )
        })
    }

    pub(crate) fn lut_filter_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.lut_filter_pipeline.get_or_init(|| {
            create_normal_source_pipeline(
                device,
                &self.bind_group_layout,
                LUT_FILTER_SHADER,
                "rizum_clip_lut_filter_shader",
                "rizum_clip_lut_filter_pipeline",
                "rizum_clip_lut_filter_pipeline_layout",
            )
        })
    }

    pub(crate) fn raster_source_pipeline(
        &self,
        device: &wgpu::Device,
        blend_mode: GpuRasterBlendMode,
    ) -> &wgpu::RenderPipeline {
        match blend_mode {
            GpuRasterBlendMode::Normal => self.alpha_pipeline(device),
            GpuRasterBlendMode::AddGlow => self.add_glow_pipeline(device),
            GpuRasterBlendMode::ColorDodge => self.color_dodge_pipeline(device),
            GpuRasterBlendMode::ColorBurn => self.color_burn_pipeline(device),
            GpuRasterBlendMode::GlowDodge => self.glow_dodge_pipeline(device),
            _ => self.standard_blend_pipeline(device),
        }
    }

    pub(crate) fn clipped_source_pipeline(
        &self,
        device: &wgpu::Device,
        blend_mode: GpuRasterBlendMode,
    ) -> &wgpu::RenderPipeline {
        match blend_mode {
            GpuRasterBlendMode::AddGlow
            | GpuRasterBlendMode::ColorBurn
            | GpuRasterBlendMode::ColorDodge
            | GpuRasterBlendMode::GlowDodge => self.clipped_byte_pipeline(device),
            _ => self.clipped_pipeline(device),
        }
    }
}

pub(crate) const WHITE_TRANSPARENT: wgpu::Color = wgpu::Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.0,
};

fn create_normal_source_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rizum_clip_normal_source_bind_group_layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn create_normal_source_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    shader_source: &'static str,
    shader_label: &'static str,
    pipeline_label: &'static str,
    pipeline_layout_label: &'static str,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(pipeline_layout_label),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(shader_label),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(pipeline_label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

pub(crate) fn create_rgba8_texture(
    device: &wgpu::Device,
    label: &'static str,
    size: CanvasSize,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    })
}

pub(super) fn raster_texture_view(
    cache: &GpuRasterResourceCache,
    source: GpuNormalRasterSource,
) -> Result<wgpu::TextureView, GpuRenderError> {
    Ok(cache
        .resource(source.key)
        .ok_or(GpuRenderError::MissingRasterResource {
            layer_id: source.key.layer_id,
            render_mipmap_id: source.key.render_mipmap_id,
        })?
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default()))
}

pub(super) fn mask_texture_view(
    mask_cache: Option<&GpuMaskResourceCache>,
    source: GpuNormalRasterSource,
    fallback_texture: &wgpu::Texture,
) -> Result<wgpu::TextureView, GpuRenderError> {
    source_mask_texture_view(mask_cache, source.mask_key, fallback_texture)
}

pub(super) fn source_mask_texture_view(
    mask_cache: Option<&GpuMaskResourceCache>,
    mask_key: Option<GpuMaskResourceKey>,
    fallback_texture: &wgpu::Texture,
) -> Result<wgpu::TextureView, GpuRenderError> {
    let Some(mask_key) = mask_key else {
        return Ok(fallback_texture.create_view(&wgpu::TextureViewDescriptor::default()));
    };
    Ok(mask_cache
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })?
        .resource(mask_key)
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })?
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default()))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_normal_source_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    dest_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 64],
    label: &'static str,
) {
    encode_normal_source_pass_with_load(
        device,
        encoder,
        pipeline,
        bind_group_layout,
        source_view,
        dest_view,
        mask_view,
        output_view,
        uniform_bytes,
        label,
        wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_normal_source_pass_scissored(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    dest_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 64],
    label: &'static str,
    scissor: CanvasRect,
) {
    encode_normal_source_pass_with_load(
        device,
        encoder,
        pipeline,
        bind_group_layout,
        source_view,
        dest_view,
        mask_view,
        output_view,
        uniform_bytes,
        label,
        wgpu::LoadOp::Load,
        Some(scissor),
    );
}

#[allow(clippy::too_many_arguments)]
fn encode_normal_source_pass_with_load(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    dest_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 64],
    label: &'static str,
    load: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<CanvasRect>,
) {
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rizum_clip_normal_source_uniform"),
        size: uniform_bytes.len() as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: true,
    });
    {
        let mut mapped = uniform.slice(..).get_mapped_range_mut();
        mapped.copy_from_slice(&uniform_bytes);
    }
    uniform.unmap();
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rizum_clip_normal_source_bind_group"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(dest_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: uniform.as_entire_binding(),
            },
        ],
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    if let Some(scissor) = scissor {
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
    }
    pass.draw(0..3, 0..1);
}

pub(crate) fn clear_rgba8_texture(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    label: &'static str,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
        multiview_mask: None,
    });
}
