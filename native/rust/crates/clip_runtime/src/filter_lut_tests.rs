use std::path::Path;

use clip_model::LayerId;

use super::{
    FILTER_TYPE_BRIGHTNESS_CONTRAST, FILTER_TYPE_COLOR_BALANCE, FILTER_TYPE_GRADIENT_MAP,
    FILTER_TYPE_HSL, FILTER_TYPE_INVERT, FILTER_TYPE_LEVEL_CORRECTION, FILTER_TYPE_POSTERIZATION,
    FILTER_TYPE_THRESHOLD, PlannedLutFilterMode, lut_filter_rgba,
};

#[test]
fn brightness_contrast_lut_matches_python_formula_anchors() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&20i32.to_be_bytes());
    payload.extend_from_slice(&(-10i32).to_be_bytes());

    let (name, mode, lut) =
        lut_filter_rgba(FILTER_TYPE_BRIGHTNESS_CONTRAST, &payload).expect("build LUT");

    assert_eq!(name, "BrightnessContrast");
    assert!(matches!(mode, PlannedLutFilterMode::ToneCurveRgb));
    for (input, expected) in [
        (0usize, 10u8),
        (20, 10),
        (64, 51),
        (128, 110),
        (200, 176),
        (255, 227),
    ] {
        assert_eq!(&lut[input * 4..input * 4 + 3], [expected; 3].as_slice());
    }
}

#[test]
fn level_correction_lut_matches_python_formula_anchors() {
    let payload = level_payload([0, 20000, 65535, 0, 65535]);

    let (name, mode, lut) =
        lut_filter_rgba(FILTER_TYPE_LEVEL_CORRECTION, &payload).expect("build LUT");

    assert_eq!(name, "LevelCorrection");
    assert!(matches!(mode, PlannedLutFilterMode::ToneCurveRgb));
    for (input, expected) in [
        (0usize, 0u8),
        (25, 5),
        (64, 24),
        (128, 78),
        (192, 157),
        (230, 214),
        (255, 255),
    ] {
        assert_eq!(&lut[input * 4..input * 4 + 3], [expected; 3].as_slice());
    }
}

#[test]
fn invert_and_posterization_luts_match_python_formula_anchors() {
    let (invert_name, invert_mode, invert_lut) =
        lut_filter_rgba(FILTER_TYPE_INVERT, &[]).expect("build invert LUT");
    assert_eq!(invert_name, "Invert");
    assert!(matches!(invert_mode, PlannedLutFilterMode::ToneCurveRgb));
    for (input, expected) in [(0usize, 255u8), (64, 191), (255, 0)] {
        assert_eq!(
            &invert_lut[input * 4..input * 4 + 3],
            [expected; 3].as_slice()
        );
    }

    let payload = 4i32.to_be_bytes();
    let (posterize_name, posterize_mode, posterize_lut) =
        lut_filter_rgba(FILTER_TYPE_POSTERIZATION, &payload).expect("build posterize LUT");
    assert_eq!(posterize_name, "Posterization");
    assert!(matches!(posterize_mode, PlannedLutFilterMode::ToneCurveRgb));
    for (input, expected) in [
        (0usize, 0u8),
        (63, 0),
        (64, 85),
        (127, 85),
        (128, 170),
        (191, 170),
        (192, 255),
        (255, 255),
    ] {
        assert_eq!(
            &posterize_lut[input * 4..input * 4 + 3],
            [expected; 3].as_slice()
        );
    }
}

#[test]
fn threshold_lut_matches_python_formula_anchors() {
    let payload = 128i32.to_be_bytes();
    let (name, mode, lut) =
        lut_filter_rgba(FILTER_TYPE_THRESHOLD, &payload).expect("build threshold LUT");

    assert_eq!(name, "Threshold");
    assert!(matches!(mode, PlannedLutFilterMode::ThresholdLum));
    for (input, expected) in [(0usize, 0u8), (127, 0), (128, 255), (255, 255)] {
        assert_eq!(&lut[input * 4..input * 4 + 3], [expected; 3].as_slice());
    }
}

#[test]
fn hsl_filter_parses_sqlite_payload_scaling() {
    let payload = hsl_payload(30, -25, 25);
    let (name, mode, lut) = lut_filter_rgba(FILTER_TYPE_HSL, &payload).expect("build HSL filter");

    assert_eq!(name, "HueSaturationLuminosity");
    let PlannedLutFilterMode::Hsl {
        hue_turns,
        saturation_delta,
        luminosity_delta,
    } = mode
    else {
        panic!("HSL filter should use the HSL GPU mode");
    };
    assert_eq!(hue_turns, 1.0 / 12.0);
    assert_eq!(saturation_delta, -0.25);
    assert_eq!(luminosity_delta, 0.25);
    for input in [0usize, 64, 128, 255] {
        assert_eq!(
            &lut[input * 4..input * 4 + 4],
            [input as u8, input as u8, input as u8, 255].as_slice()
        );
    }
}

#[test]
fn color_balance_lut_matches_preserve_luminosity_python_formula_anchors() {
    let payload = color_balance_payload([1, 0, 0, 0, 43, -48, 48, 0, 0, 0]);

    let (name, mode, lut) =
        lut_filter_rgba(FILTER_TYPE_COLOR_BALANCE, &payload).expect("build color balance LUT");

    assert_eq!(name, "ColorBalance");
    assert!(matches!(mode, PlannedLutFilterMode::ToneCurveRgb));
    for (input, expected) in [
        (0usize, [0, 0, 0]),
        (64, [81, 30, 84]),
        (128, [144, 88, 146]),
        (192, [201, 164, 203]),
        (226, [231, 212, 231]),
        (255, [255, 255, 255]),
    ] {
        assert_eq!(&lut[input * 4..input * 4 + 3], expected.as_slice());
    }
}

#[test]
fn color_balance_lut_matches_normal_python_formula_anchors() {
    let payload = color_balance_payload([0, -20, 10, 35, 40, -30, 15, 25, 0, -50]);

    let (name, mode, lut) =
        lut_filter_rgba(FILTER_TYPE_COLOR_BALANCE, &payload).expect("build color balance LUT");

    assert_eq!(name, "ColorBalance");
    assert!(matches!(mode, PlannedLutFilterMode::ToneCurveRgb));
    for (input, expected) in [
        (0usize, [0, 0, 0]),
        (32, [29, 23, 36]),
        (64, [76, 52, 70]),
        (128, [152, 115, 133]),
        (192, [218, 184, 195]),
        (226, [250, 222, 228]),
        (255, [255, 255, 255]),
    ] {
        assert_eq!(&lut[input * 4..input * 4 + 3], expected.as_slice());
    }
}

#[test]
fn malformed_lut_filter_payloads_fail_closed() {
    assert!(lut_filter_rgba(FILTER_TYPE_BRIGHTNESS_CONTRAST, &[0; 7]).is_none());
    assert!(lut_filter_rgba(FILTER_TYPE_LEVEL_CORRECTION, &[0; 0x3f]).is_none());
    assert!(lut_filter_rgba(FILTER_TYPE_HSL, &[0; 11]).is_none());
    assert!(lut_filter_rgba(FILTER_TYPE_COLOR_BALANCE, &[0; 39]).is_none());
    assert!(lut_filter_rgba(FILTER_TYPE_POSTERIZATION, &[0; 3]).is_none());
    assert!(lut_filter_rgba(FILTER_TYPE_THRESHOLD, &[0; 3]).is_none());
    assert!(lut_filter_rgba(99, &[]).is_none());
}

#[test]
fn gradient_map_lut_matches_test_gradiation_baseline_anchors() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Gradiation.clip");
    let container =
        clip_file::container::ClipContainer::open(path).expect("open Test_Gradiation.clip");
    let filter = clip_file::metadata::read_filter_layer_source_from_sqlite(
        container.sqlite_bytes(),
        LayerId(6),
    )
    .expect("read gradient map payload");
    let (name, mode, lut) =
        lut_filter_rgba(filter.filter_type, &filter.payload).expect("build gradient map LUT");

    assert_eq!(name, "GradientMap");
    assert!(matches!(mode, PlannedLutFilterMode::GradientMapLum));
    assert_eq!(filter.filter_type, FILTER_TYPE_GRADIENT_MAP);
    for (input, expected) in [
        (0usize, [77, 96, 126]),
        (1, [98, 100, 123]),
        (64, [186, 132, 133]),
        (128, [151, 174, 180]),
        (192, [198, 215, 201]),
        (255, [255, 253, 236]),
    ] {
        assert_eq!(&lut[input * 4..input * 4 + 3], expected.as_slice());
    }
}

fn level_payload(group: [u16; 5]) -> Vec<u8> {
    let mut payload = vec![0u8; 0x40];
    for (index, value) in group.iter().enumerate() {
        payload[index * 2..index * 2 + 2].copy_from_slice(&value.to_be_bytes());
    }
    payload
}

fn color_balance_payload(values: [i32; 10]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(40);
    for value in values {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload
}

fn hsl_payload(hue: i32, saturation: i32, luminosity: i32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    for value in [hue, saturation, luminosity] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload
}
