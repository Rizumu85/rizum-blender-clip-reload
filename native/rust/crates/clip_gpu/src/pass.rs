#[path = "pass_clipping.rs"]
mod pass_clipping;
#[path = "pass_container.rs"]
mod pass_container;
#[path = "pass_normal.rs"]
mod pass_normal;
#[path = "pass_normal_encode.rs"]
mod pass_normal_encode;
#[path = "pass_pipeline.rs"]
mod pass_pipeline;
#[path = "pass_through.rs"]
mod pass_through;

use clip_model::CanvasSize;
pub(crate) use pass_pipeline::{
    NormalStackPipelines, WHITE_TRANSPARENT, create_rgba8_texture, encode_normal_source_pass,
    encode_normal_source_pass_scissored,
};

use crate::resource::GpuRasterResource;
use crate::shaders::COPY_RASTER_SHADER;
use crate::types::{GpuRasterDrawOutput, GpuRasterStackOutput};
use crate::{GpuRasterResourceCache, GpuRasterResourceKey, GpuRenderError, GpuRenderer};

impl GpuRenderer {
    pub fn draw_raster_resource_to_rgba8(
        &self,
        cache: &GpuRasterResourceCache,
        key: GpuRasterResourceKey,
    ) -> Result<GpuRasterDrawOutput, GpuRenderError> {
        let output = self.draw_raster_stack_to_rgba8(cache, &[key])?;
        let info = output.drawn_resources[0];
        Ok(GpuRasterDrawOutput {
            resource_info: info,
            size: output.size,
            pixels: output.pixels,
        })
    }

    pub fn draw_raster_stack_to_rgba8(
        &self,
        cache: &GpuRasterResourceCache,
        keys: &[GpuRasterResourceKey],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        let StackResources {
            size: output_size,
            resources,
        } = stack_resources(cache, keys)?;

        let output_texture = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("rizum_clip_single_resource_output_texture"),
                size: wgpu::Extent3d {
                    width: output_size.width,
                    height: output_size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group_layout =
            self.context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("rizum_clip_copy_raster_bind_group_layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    }],
                });
        let pipeline_layout =
            self.context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("rizum_clip_copy_raster_pipeline_layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });
        let shader = self
            .context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rizum_clip_copy_raster_shader"),
                source: wgpu::ShaderSource::Wgsl(COPY_RASTER_SHADER.into()),
            });
        let pipeline =
            self.context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("rizum_clip_copy_raster_pipeline"),
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
                });

        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_copy_raster_encoder"),
                });
        for (index, resource) in resources.iter().enumerate() {
            let source_view = resource
                .texture()
                .create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self
                .context
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rizum_clip_copy_raster_bind_group"),
                    layout: &bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source_view),
                    }],
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rizum_clip_copy_raster_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if index == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.context.queue.submit([encoder.finish()]);

        let pixels =
            self.read_texture_rgba8(&output_texture, output_size.width, output_size.height)?;
        Ok(GpuRasterStackOutput {
            drawn_resources: resources.iter().map(|resource| resource.info()).collect(),
            size: output_size,
            pixels,
        })
    }
}

pub(super) struct StackResources<'a> {
    pub(super) size: CanvasSize,
    pub(super) resources: Vec<&'a GpuRasterResource>,
}

fn stack_resources<'a>(
    cache: &'a GpuRasterResourceCache,
    keys: &[GpuRasterResourceKey],
) -> Result<StackResources<'a>, GpuRenderError> {
    let Some(first_key) = keys.first().copied() else {
        return Err(GpuRenderError::EmptyRasterStack);
    };
    let first_resource =
        cache
            .resource(first_key)
            .ok_or(GpuRenderError::MissingRasterResource {
                layer_id: first_key.layer_id,
                render_mipmap_id: first_key.render_mipmap_id,
            })?;
    let output_size = first_resource.info().size;
    let mut resources = Vec::with_capacity(keys.len());
    for key in keys {
        let resource = cache
            .resource(*key)
            .ok_or(GpuRenderError::MissingRasterResource {
                layer_id: key.layer_id,
                render_mipmap_id: key.render_mipmap_id,
            })?;
        let info = resource.info();
        if info.size != output_size {
            return Err(GpuRenderError::RasterResourceSizeMismatch {
                layer_id: info.key.layer_id,
                expected: output_size,
                actual: info.size,
            });
        }
        resources.push(resource);
    }
    Ok(StackResources {
        size: output_size,
        resources,
    })
}

#[cfg(test)]
#[path = "pass_tests.rs"]
mod pass_tests;
