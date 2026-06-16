use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

use clip_model::Rect;

use super::{
    decode_external_tile_blocks, inspect_external_tile_blocks,
    visit_external_non_empty_tile_block_selection, visit_external_tile_block_selection,
};
use crate::tile_region::tile_block_selection_for_region;

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

#[test]
fn selected_tile_decode_stops_after_last_wanted_block() {
    let body = external_body(&[
        compressed_data_block(&[1, 2, 3, 4]),
        corrupt_compressed_data_block(4),
    ]);

    let blocks = decode_external_tile_blocks(&body, 9, 4, 2, &[0]).unwrap();

    assert_eq!(blocks.external_id, "external");
    assert_eq!(blocks.blocks.len(), 1);
    assert_eq!(blocks.blocks[0].tile_index, 0);
    assert_eq!(blocks.blocks[0].bytes, vec![1, 2, 3, 4]);
}

#[test]
fn selected_tile_visitor_reuses_region_selection_without_inflating_skipped_blocks() {
    let body = external_body(&[
        corrupt_compressed_data_block(4),
        compressed_data_block(&[5, 6, 7, 8]),
        corrupt_compressed_data_block(4),
    ]);
    let selection = tile_block_selection_for_region(512, 256, Rect::new(256, 0, 64, 64)).unwrap();
    let mut visited = Vec::new();

    let external_id =
        visit_external_tile_block_selection(&body, 9, 4, 2, selection, |tile_index, bytes| {
            visited.push((tile_index, bytes.to_vec()));
            Ok(())
        })
        .unwrap();

    assert_eq!(external_id, "external");
    assert_eq!(visited, vec![(1, vec![5, 6, 7, 8])]);
}

#[test]
fn block_stats_do_not_inflate_compressed_blocks() {
    let body = external_body(&[
        compressed_data_block(&[1, 2, 3, 4]),
        corrupt_compressed_data_block(4),
        empty_data_block(4),
    ]);

    let stats = inspect_external_tile_blocks(&body, 4, 3, 3).unwrap();

    assert_eq!(stats.external_id, "external");
    assert_eq!(stats.block_count, 3);
    assert_eq!(stats.compressed_block_count, 2);
    assert_eq!(stats.empty_block_count, 1);
    assert_eq!(stats.uncompressed_bytes, 12);
    assert!(stats.compressed_bytes > 0);
    let bounds = stats
        .compressed_tile_bounds
        .expect("compressed blocks should have bounds");
    assert_eq!(bounds.tile_x, 0);
    assert_eq!(bounds.tile_y, 0);
    assert_eq!(bounds.tile_width, 2);
    assert_eq!(bounds.tile_height, 1);
}

#[test]
fn non_empty_tile_visitor_skips_empty_blocks_without_inflating_them() {
    let body = external_body(&[compressed_data_block(&[1, 2, 3, 4]), empty_data_block(4)]);
    let selection = tile_block_selection_for_region(512, 256, Rect::new(0, 0, 512, 256)).unwrap();
    let mut visited = Vec::new();

    let external_id = visit_external_non_empty_tile_block_selection(
        &body,
        4,
        2,
        selection,
        |tile_index, bytes| {
            visited.push((tile_index, bytes.to_vec()));
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(external_id, "external");
    assert_eq!(visited, vec![(0, vec![1, 2, 3, 4])]);
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
