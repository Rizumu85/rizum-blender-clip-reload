use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

use super::decode_external_tile_blocks;

#[test]
fn selected_tile_decode_skips_unwanted_compressed_blocks() {
    let body = external_body(&[
        compressed_data_block(&[1, 2, 3, 4]),
        corrupt_compressed_data_block(4),
        empty_data_block(4),
    ]);

    let blocks = decode_external_tile_blocks(&body, 9, 4, 3, &[2, 0]).unwrap();

    assert_eq!(blocks.external_id, "external");
    assert_eq!(blocks.blocks.len(), 2);
    assert_eq!(blocks.blocks[0].tile_index, 0);
    assert_eq!(blocks.blocks[0].bytes, vec![1, 2, 3, 4]);
    assert_eq!(blocks.blocks[1].tile_index, 2);
    assert_eq!(blocks.blocks[1].bytes, vec![9, 9, 9, 9]);
}

fn external_body(blocks: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    let external_id = b"external";
    body.extend_from_slice(&(external_id.len() as u64).to_be_bytes());
    body.extend_from_slice(external_id);
    body.extend_from_slice(&[0u8; 8]);
    for block in blocks {
        body.extend_from_slice(block);
    }
    body
}

fn compressed_data_block(uncompressed: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(uncompressed).unwrap();
    let compressed = encoder.finish().unwrap();
    data_block(uncompressed.len(), 1, &compressed)
}

fn corrupt_compressed_data_block(uncompressed_len: usize) -> Vec<u8> {
    data_block(uncompressed_len, 1, &[0, 1, 2, 3])
}

fn empty_data_block(uncompressed_len: usize) -> Vec<u8> {
    data_block(uncompressed_len, 0, &[])
}

fn data_block(uncompressed_len: usize, exists: u32, compressed: &[u8]) -> Vec<u8> {
    let mut inner = Vec::new();
    inner.extend_from_slice(&[0u8; 4]);
    inner.extend_from_slice(&(uncompressed_len as u32).to_be_bytes());
    inner.extend_from_slice(&[0u8; 8]);
    inner.extend_from_slice(&exists.to_be_bytes());
    if exists > 0 {
        let inner_len = compressed.len() + 4;
        inner.extend_from_slice(&(inner_len as u32).to_be_bytes());
        inner.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        inner.extend_from_slice(compressed);
    }
    named_block("BlockDataBeginChunk", &inner)
}

fn named_block(name: &str, inner: &[u8]) -> Vec<u8> {
    let mut block = Vec::new();
    block.extend_from_slice(&(inner.len() as u32).to_be_bytes());
    block.extend_from_slice(&(name.encode_utf16().count() as u32).to_be_bytes());
    for unit in name.encode_utf16() {
        block.extend_from_slice(&unit.to_be_bytes());
    }
    block.extend_from_slice(inner);
    block
}
