use std::path::Path;

use crate::container::ClipContainer;
use crate::external::decode_external_tile_blob;
use crate::metadata::{
    read_filter_layer_source_from_sqlite, read_layer_graph_records_from_sqlite,
    read_raster_layer_source_from_sqlite,
};
use crate::tiles::{decode_rgba_tiles, rgba_tile_blob_len};

use super::*;

#[test]
fn reads_test_clipping_summary() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let summary = read_summary(path).expect("read Test_Clipping.clip summary");

    assert_eq!(summary.canvas, CanvasSize::new(512, 512));
    assert_eq!(summary.root_layer_id, LayerId(2));
    assert_eq!(summary.layer_count, 4);
    assert_eq!(summary.external_data_count, 7);
}

#[test]
fn reads_test_clipping_layer_graph_records() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let container = ClipContainer::open(path).expect("open Test_Clipping.clip container");
    let records =
        read_layer_graph_records_from_sqlite(container.sqlite_bytes()).expect("read layers");

    assert_eq!(records.len(), 4);
    assert_eq!(records[0].id, LayerId(2));
    assert_eq!(records[0].name, "");
    assert_eq!(records[0].kind, clip_model::LayerKind::Folder);
    assert_eq!(records[0].first_child_layer_id, Some(LayerId(4)));
    assert_eq!(records[0].next_layer_id, None);

    assert_eq!(records[1].id, LayerId(4));
    assert_eq!(records[1].name, "Paper");
    assert_eq!(records[1].kind, clip_model::LayerKind::Paper);
    assert_eq!(records[1].next_layer_id, Some(LayerId(10)));
    assert_eq!(
        records[1].paper_color,
        Some(clip_model::Rgba8 {
            r: 226,
            g: 226,
            b: 226,
            a: 255,
        }),
    );

    assert_eq!(records[2].id, LayerId(10));
    assert_eq!(records[2].name, "Layer 1");
    assert_eq!(records[2].kind, clip_model::LayerKind::Raster);
    assert_eq!(records[2].next_layer_id, Some(LayerId(11)));
    assert_eq!(records[2].render_mipmap_id, Some(15));

    assert_eq!(records[3].id, LayerId(11));
    assert_eq!(records[3].name, "Layer 2");
    assert_eq!(records[3].kind, clip_model::LayerKind::Raster);
    assert_eq!(records[3].next_layer_id, None);
    assert_eq!(records[3].render_mipmap_id, Some(16));
}

#[test]
fn decodes_test_clipping_layer_10_rgba_tiles() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let container = ClipContainer::open(path).expect("open Test_Clipping.clip container");
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )
    .expect("read summary");
    let source =
        read_raster_layer_source_from_sqlite(container.sqlite_bytes(), LayerId(10), summary.canvas)
            .expect("read raster layer source");

    assert_eq!(source.layer.kind, clip_model::LayerKind::Raster);
    assert!(source.layer.visibility.is_visible());
    assert_eq!(source.render_mipmap_id, 15);
    assert_eq!(source.offscreen_id, 62);
    assert_eq!(
        source.external_id,
        "extrnlid7A4545CCDE9D4E579B1230B4DB88B130",
    );
    assert_eq!(source.pixel_size, CanvasSize::new(512, 512));
    assert_eq!(source.color_type, Some(0));
    assert_eq!(source.offset_x, 0);
    assert_eq!(source.offset_y, 0);

    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))
        .expect("external data body");
    let expected_len =
        rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height).unwrap();
    let blob =
        decode_external_tile_blob(body, 0, Some(expected_len)).expect("decode external tile blob");
    assert_eq!(blob.external_id, source.external_id);
    assert_eq!(blob.bytes.len(), 1_310_720);

    let image = decode_rgba_tiles(
        &blob.bytes,
        source.pixel_size.width,
        source.pixel_size.height,
    )
    .expect("decode rgba tiles");
    assert_eq!(image.width, 512);
    assert_eq!(image.height, 512);

    assert_eq!(pixel_at(&image.pixels, 512, 0, 0), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&image.pixels, 512, 100, 100), [0, 0, 0, 0]);

    let mut alpha_count = 0usize;
    let mut sums = [0u64; 4];
    for pixel in image.pixels.chunks_exact(4) {
        if pixel[3] > 0 {
            alpha_count += 1;
        }
        for channel in 0..4 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    assert_eq!(alpha_count, 37_151);
    assert_eq!(sums, [8_507_579, 5_832_707, 5_832_707, 8_933_976]);
}

#[test]
fn reads_ref_terra_render_offsets() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Ref_Terra404_Live2D.clip");
    if !path.exists() {
        eprintln!("skip missing private fixture: {}", path.display());
        return;
    }
    let container = ClipContainer::open(path).expect("open Ref_Terra404_Live2D.clip");
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )
    .expect("read summary");

    let layer_4 =
        read_raster_layer_source_from_sqlite(container.sqlite_bytes(), LayerId(4), summary.canvas)
            .expect("read raster layer 4 source");
    let layer_5 =
        read_raster_layer_source_from_sqlite(container.sqlite_bytes(), LayerId(5), summary.canvas)
            .expect("read raster layer 5 source");

    assert_eq!(layer_4.offset_x, -768);
    assert_eq!(layer_4.offset_y, 0);
    assert_eq!(layer_4.color_type, Some(0));
    assert_eq!(layer_5.offset_x, -1280);
    assert_eq!(layer_5.offset_y, 0);
    assert_eq!(layer_5.color_type, Some(0));
}

#[test]
fn public_api_decodes_test_clipping_layer_11() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let image = read_raster_layer_rgba(path, LayerId(11)).expect("decode layer 11");

    assert_eq!(image.width, 512);
    assert_eq!(image.height, 512);
    assert_eq!(pixel_at(&image.pixels, 512, 0, 0), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&image.pixels, 512, 100, 100), [80, 70, 229, 255],);
    assert_eq!(pixel_at(&image.pixels, 512, 300, 300), [80, 70, 229, 255],);

    let alpha_count = image
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 0)
        .count();
    assert_eq!(alpha_count, 196_553);
}

#[test]
fn public_api_decodes_test_mask_layer_5_mask() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Mask.clip");
    let mask = read_layer_mask_alpha(path, LayerId(5)).expect("decode layer 5 mask");

    assert_eq!(mask.width, 512);
    assert_eq!(mask.height, 512);
    assert_eq!(mask.pixels[0], 255);
    assert_eq!(mask.pixels[100 * 512 + 100], 255);
    assert_eq!(mask.pixels[256 * 512 + 256], 255);

    let nonzero = mask.pixels.iter().filter(|value| **value > 0).count();
    let sum: u64 = mask.pixels.iter().map(|value| u64::from(*value)).sum();
    assert_eq!(nonzero, 227_309);
    assert_eq!(sum, 57_888_516);
}

#[test]
fn public_api_decodes_test_grayscale_layer_5() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_ Grayscale.clip");
    let image = read_raster_layer_rgba(path, LayerId(5)).expect("decode grayscale layer 5");

    assert_eq!(image.width, 1024);
    assert_eq!(image.height, 1024);
    assert_eq!(pixel_at(&image.pixels, 1024, 0, 0), [129, 129, 129, 38]);
    assert_eq!(pixel_at(&image.pixels, 1024, 100, 100), [129, 129, 129, 34],);
    assert_eq!(pixel_at(&image.pixels, 1024, 300, 300), [129, 129, 129, 26],);
    assert_eq!(pixel_at(&image.pixels, 1024, 512, 512), [129, 129, 129, 18],);
    let (nonzero, sums) = rgba_stats(&image.pixels);
    assert_eq!(nonzero, 701_073);
    assert_eq!(sums, [93_813_265, 93_813_265, 93_813_265, 70_251_599]);
}

#[test]
fn public_api_decodes_test_monochrome_layer_7() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Monochrome.clip");
    let image = read_raster_layer_rgba(path, LayerId(7)).expect("decode monochrome layer 7");

    assert_eq!(image.width, 1024);
    assert_eq!(image.height, 1024);
    assert_eq!(pixel_at(&image.pixels, 1024, 0, 0), [255, 255, 255, 255],);
    assert_eq!(
        pixel_at(&image.pixels, 1024, 100, 100),
        [255, 255, 255, 255],
    );
    assert_eq!(
        pixel_at(&image.pixels, 1024, 300, 300),
        [255, 255, 255, 255],
    );
    assert_eq!(pixel_at(&image.pixels, 1024, 512, 512), [0, 0, 0, 0]);
    let (nonzero, sums) = rgba_stats(&image.pixels);
    assert_eq!(nonzero, 422_394);
    assert_eq!(sums, [104_312_850, 104_312_850, 104_312_850, 107_710_470],);
}

#[test]
fn reads_test_tone_curve_filter_payload() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_ToneCurve.clip");
    let container = ClipContainer::open(path).expect("open Test_ToneCurve.clip container");
    let filter = read_filter_layer_source_from_sqlite(container.sqlite_bytes(), LayerId(6))
        .expect("read tone curve filter source");

    assert_eq!(filter.layer_id, LayerId(6));
    assert_eq!(filter.filter_type, 3);
    assert_eq!(filter.payload.len(), 4160);
}

#[test]
fn reads_test_gradiation_filter_payload() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Gradiation.clip");
    let container = ClipContainer::open(path).expect("open Test_Gradiation.clip container");
    let filter = read_filter_layer_source_from_sqlite(container.sqlite_bytes(), LayerId(6))
        .expect("read gradient map filter source");

    assert_eq!(filter.layer_id, LayerId(6));
    assert_eq!(filter.filter_type, 9);
    assert_eq!(filter.payload.len(), 280);
}

fn pixel_at(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let offset = (y * width + x) * 4;
    pixels[offset..offset + 4].try_into().unwrap()
}

fn rgba_stats(pixels: &[u8]) -> (usize, [u64; 4]) {
    let mut nonzero = 0usize;
    let mut sums = [0u64; 4];
    for pixel in pixels.chunks_exact(4) {
        if pixel[3] > 0 {
            nonzero += 1;
        }
        for channel in 0..4 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    (nonzero, sums)
}
