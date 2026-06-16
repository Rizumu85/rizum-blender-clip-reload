use crate::GpuRenderError;
use crate::pass::WHITE_TRANSPARENT;
use crate::stream_bounds::CanvasRect;

pub(crate) fn create_lut_filter_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    lut_rgba: &[u8],
) -> Result<wgpu::Texture, GpuRenderError> {
    if lut_rgba.len() != 256 * 4 {
        return Err(GpuRenderError::InvalidToneCurveLutLength {
            expected: 256 * 4,
            actual: lut_rgba.len(),
        });
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rizum_clip_lut_filter_texture"),
        size: wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        lut_rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * 4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    Ok(texture)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_lut_filter_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    lut_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 16],
    label: &'static str,
) {
    encode_lut_filter_pass_with_load(
        device,
        encoder,
        pipeline,
        bind_group_layout,
        source_view,
        mask_view,
        lut_view,
        output_view,
        uniform_bytes,
        label,
        wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_lut_filter_pass_scissored(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    lut_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 16],
    label: &'static str,
    scissor: CanvasRect,
) {
    encode_lut_filter_pass_with_load(
        device,
        encoder,
        pipeline,
        bind_group_layout,
        source_view,
        mask_view,
        lut_view,
        output_view,
        uniform_bytes,
        label,
        wgpu::LoadOp::Load,
        Some(scissor),
    );
}

#[allow(clippy::too_many_arguments)]
fn encode_lut_filter_pass_with_load(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    lut_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 16],
    label: &'static str,
    load: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<CanvasRect>,
) {
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rizum_clip_lut_filter_uniform"),
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
        label: Some("rizum_clip_lut_filter_bind_group"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(lut_view),
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
