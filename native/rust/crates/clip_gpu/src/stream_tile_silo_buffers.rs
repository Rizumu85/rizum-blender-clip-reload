use crate::stream_tile_event::TileEventProgram;
use crate::stream_tile_silo_plan::TILE_SIZE;

pub(crate) struct TileEventStorageBuffers {
    pub(crate) headers: wgpu::Buffer,
    pub(crate) raster_payloads: wgpu::Buffer,
    pub(crate) filter_payloads: wgpu::Buffer,
}

pub(crate) fn create_tile_event_storage_buffers(
    device: &wgpu::Device,
    header_label: &'static str,
    raster_payload_label: &'static str,
    program: &TileEventProgram,
) -> TileEventStorageBuffers {
    let headers = create_u32_storage_buffer(device, header_label, &program.header_words());
    let raster_payloads = create_u32_storage_buffer(
        device,
        raster_payload_label,
        &program.raster_payload_words(),
    );
    let filter_payloads = create_u32_storage_buffer(
        device,
        "rizum_clip_tile_silo_filter_payloads",
        &program.filter_payload_words(),
    );
    TileEventStorageBuffers {
        headers,
        raster_payloads,
        filter_payloads,
    }
}

pub(crate) fn create_u32_storage_buffer(
    device: &wgpu::Device,
    label: &'static str,
    values: &[u32],
) -> wgpu::Buffer {
    if values.is_empty() {
        return create_buffer_with_bytes(
            device,
            label,
            wgpu::BufferUsages::STORAGE,
            &u32_bytes(&[0]),
        );
    }
    create_buffer_with_bytes(
        device,
        label,
        wgpu::BufferUsages::STORAGE,
        &u32_bytes(values),
    )
}

pub(crate) fn create_params_buffer(
    device: &wgpu::Device,
    target_origin: (i32, i32),
    tile_cols: u32,
) -> wgpu::Buffer {
    create_params_buffer_with_mode_and_resolve(device, target_origin, tile_cols, 0, 0, 0)
}

pub(crate) fn create_params_buffer_with_mode(
    device: &wgpu::Device,
    target_origin: (i32, i32),
    tile_cols: u32,
    mode: u32,
) -> wgpu::Buffer {
    create_params_buffer_with_mode_and_resolve(device, target_origin, tile_cols, mode, 0, 0)
}

pub(crate) fn create_params_buffer_with_mode_and_resolve(
    device: &wgpu::Device,
    target_origin: (i32, i32),
    tile_cols: u32,
    mode: u32,
    resolve_blend_kind: u32,
    base_event_count: u32,
) -> wgpu::Buffer {
    let mut bytes = Vec::with_capacity(48);
    bytes.extend_from_slice(&target_origin.0.to_ne_bytes());
    bytes.extend_from_slice(&target_origin.1.to_ne_bytes());
    bytes.extend_from_slice(&TILE_SIZE.to_ne_bytes());
    bytes.extend_from_slice(&tile_cols.to_ne_bytes());
    bytes.extend_from_slice(&mode.to_ne_bytes());
    bytes.extend_from_slice(&resolve_blend_kind.to_ne_bytes());
    bytes.extend_from_slice(&base_event_count.to_ne_bytes());
    for _ in 0..5 {
        bytes.extend_from_slice(&0u32.to_ne_bytes());
    }
    create_buffer_with_bytes(
        device,
        "rizum_clip_tile_silo_params",
        wgpu::BufferUsages::UNIFORM,
        &bytes,
    )
}

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn create_buffer_with_bytes(
    device: &wgpu::Device,
    label: &'static str,
    usage: wgpu::BufferUsages,
    bytes: &[u8],
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.len() as wgpu::BufferAddress,
        usage,
        mapped_at_creation: true,
    });
    {
        let mut mapped = buffer.slice(..).get_mapped_range_mut();
        mapped.copy_from_slice(bytes);
    }
    buffer.unmap();
    buffer
}
