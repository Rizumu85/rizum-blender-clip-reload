use crate::stream_tile_silo_plan::TILE_SIZE;

pub(crate) fn create_u32_storage_buffer(
    device: &wgpu::Device,
    label: &'static str,
    values: &[u32],
) -> wgpu::Buffer {
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
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&target_origin.0.to_ne_bytes());
    bytes.extend_from_slice(&target_origin.1.to_ne_bytes());
    bytes.extend_from_slice(&TILE_SIZE.to_ne_bytes());
    bytes.extend_from_slice(&tile_cols.to_ne_bytes());
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
