use std::{fs::File, io::BufWriter, path::PathBuf};

use skia_safe::{Color, Font, FontHinting, FontMgr, FontStyle, Paint, Point, surfaces};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let output = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("skia_text_probe.png"));
    let font_path = args.get(2).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(
            r"C:\Users\Rizum\AppData\Local\Microsoft\Windows\Fonts\HarmonyOS_Sans_Bold.ttf",
        )
    });
    let size = arg_f32(&args, 3, 90.0);
    let x = arg_f32(&args, 4, 15.0);
    let baseline = arg_f32(&args, 5, 95.0);
    let skew = arg_f32(&args, 6, -0.25);

    let font_data = std::fs::read(&font_path).expect("read font");
    let typeface = FontMgr::new()
        .new_from_data(&font_data, None)
        .or_else(|| FontMgr::new().legacy_make_typeface(None, FontStyle::normal()))
        .expect("typeface");
    let mut font = Font::from_typeface(typeface, size);
    font.set_subpixel(true);
    font.set_edging(skia_safe::font::Edging::AntiAlias);
    font.set_hinting(FontHinting::Normal);
    font.set_skew_x(skew);

    let mut surface = surfaces::raster_n32_premul((200, 200)).expect("surface");
    let canvas = surface.canvas();
    canvas.clear(Color::from_argb(255, 226, 226, 226));
    let mut paint = Paint::default();
    paint.set_color(Color::from_argb(255, 39, 39, 39));
    canvas.draw_str("Test", Point::new(x, baseline), &font, &paint);

    let image = surface.image_snapshot();
    let pixmap = image.peek_pixels().expect("peek pixels");
    let info = pixmap.info();
    let pixels = pixmap.bytes().expect("pixmap bytes");
    let width = info.width() as usize;
    let height = info.height() as usize;

    let file = File::create(output).expect("create output");
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder.write_header().expect("png header");

    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        let src = &pixels[y * pixmap.row_bytes()..][..width * 4];
        for x in 0..width {
            let b = src[x * 4];
            let g = src[x * 4 + 1];
            let r = src[x * 4 + 2];
            let a = src[x * 4 + 3];
            let dst = &mut rgba[(y * width + x) * 4..][..4];
            dst.copy_from_slice(&[r, g, b, a]);
        }
    }
    png_writer.write_image_data(&rgba).expect("png data");
}

fn arg_f32(args: &[String], index: usize, default: f32) -> f32 {
    args.get(index)
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(default)
}
