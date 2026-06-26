use skia_safe::{
    Font, FontMgr, FourByteTag, GlyphId, Point, Shaper, TextBlob, TextBlobBuilder,
    shaper::{
        RunHandler,
        run_handler::{Buffer, RunInfo},
    },
};

#[derive(Debug)]
pub(crate) struct ShapedTextRun {
    pub(crate) blob: TextBlob,
    pub(crate) advance_x: f32,
    pub(crate) char_positions: Vec<(f32, f32)>,
}

pub(crate) struct ShapedTextLineRunInput<'a> {
    pub(crate) text: &'a str,
    pub(crate) font: &'a Font,
}

#[derive(Debug)]
pub(crate) struct ShapedTextLinePlan {
    pub(crate) runs: Vec<ShapedTextLineGlyphRun>,
    pub(crate) char_positions: Vec<(f32, f32)>,
    pub(crate) advance_x: f32,
}

#[derive(Debug)]
pub(crate) struct ShapedTextLineGlyphRun {
    pub(crate) input_index: usize,
    pub(crate) x: f32,
    pub(crate) blob: TextBlob,
}

pub(crate) fn shape_text_line(runs: &[ShapedTextLineRunInput<'_>]) -> Option<ShapedTextLinePlan> {
    if runs.is_empty() {
        return None;
    }
    let mut x = 0.0f32;
    let mut glyph_runs = Vec::with_capacity(runs.len());
    let mut char_positions = Vec::new();
    for (input_index, input) in runs.iter().enumerate() {
        let shaped = shape_text_run(input.text, input.font)?;
        let run_x = x;
        let ShapedTextRun {
            blob,
            advance_x,
            char_positions: run_positions,
        } = shaped;
        glyph_runs.push(ShapedTextLineGlyphRun {
            input_index,
            x: run_x,
            blob,
        });
        char_positions.extend(
            run_positions
                .into_iter()
                .map(|(start_x, end_x)| (run_x + start_x, run_x + end_x)),
        );
        x += advance_x;
    }
    Some(ShapedTextLinePlan {
        runs: glyph_runs,
        char_positions,
        advance_x: x,
    })
}

pub(crate) fn shape_text_run(text: &str, font: &Font) -> Option<ShapedTextRun> {
    if text.is_empty() {
        return None;
    }
    let shaper = Shaper::new_shape_then_wrap(FontMgr::default())?;
    let mut font_runs = Shaper::new_trivial_font_run_iterator(font, text.len());
    let mut bidi_runs = Shaper::new_bidi_run_iterator(text, 0xfe)?;
    let mut script_runs = Shaper::new_script_run_iterator(text, dominant_script_tag(text));
    let mut language_runs =
        Shaper::new_trivial_language_run_iterator(dominant_language_tag(text), text.len());

    let mut handler = CspTextRunHandler::new(text);
    shaper.shape_with_iterators(
        text,
        &mut font_runs,
        &mut bidi_runs,
        &mut script_runs,
        &mut language_runs,
        f32::MAX,
        &mut handler,
    );
    handler.finish()
}

struct CspTextRunHandler<'a> {
    text: &'a str,
    builder: TextBlobBuilder,
    current_point: Point,
    pending_glyphs: Vec<GlyphId>,
    pending_positions: Vec<Point>,
    pending_clusters: Vec<u32>,
    char_starts: Vec<usize>,
    char_x: Vec<Option<f32>>,
    advance_x: f32,
}

impl<'a> CspTextRunHandler<'a> {
    fn new(text: &'a str) -> Self {
        let char_starts = text
            .char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        let char_count = char_starts.len().saturating_sub(1);
        Self {
            text,
            builder: TextBlobBuilder::new(),
            current_point: Point::default(),
            pending_glyphs: Vec::new(),
            pending_positions: Vec::new(),
            pending_clusters: Vec::new(),
            char_starts,
            char_x: vec![None; char_count],
            advance_x: 0.0,
        }
    }

    fn finish(mut self) -> Option<ShapedTextRun> {
        let blob = self.builder.make()?;
        let positions = self.char_positions();
        Some(ShapedTextRun {
            blob,
            advance_x: self.advance_x,
            char_positions: positions,
        })
    }

    fn char_positions(&self) -> Vec<(f32, f32)> {
        let char_count = self.char_starts.len().saturating_sub(1);
        let mut starts = vec![0.0; char_count + 1];
        for index in 0..char_count {
            starts[index] = self
                .char_x
                .get(index)
                .and_then(|value| *value)
                .unwrap_or_else(|| if index == 0 { 0.0 } else { starts[index - 1] });
        }
        starts[char_count] = self.advance_x.max(*starts.last().unwrap_or(&0.0));
        for index in (0..char_count).rev() {
            if starts[index] > starts[index + 1] {
                starts[index] = starts[index + 1];
            }
        }
        (0..char_count)
            .map(|index| (starts[index], starts[index + 1]))
            .collect()
    }

    fn commit_pending_to_blob(&mut self, info: &RunInfo) {
        if info.glyph_count == 0 {
            self.current_point.x += info.advance.x;
            self.current_point.y += info.advance.y;
            self.advance_x = self.advance_x.max(self.current_point.x);
            return;
        }

        let text_range = info.utf8_range.clone();
        let text_bytes = &self.text.as_bytes()[text_range.clone()];
        let (glyphs, positions, utf8, clusters) =
            self.builder
                .alloc_run_text_pos(info.font, info.glyph_count, text_bytes.len(), None);
        glyphs.copy_from_slice(&self.pending_glyphs);
        positions.copy_from_slice(&self.pending_positions);
        utf8.copy_from_slice(text_bytes);
        clusters.copy_from_slice(&self.pending_clusters);

        let run_start = text_range.start;
        let run_end = text_range.end;
        for (position, cluster) in self
            .pending_positions
            .iter()
            .zip(self.pending_clusters.iter().copied())
        {
            let byte = normalize_cluster_byte(cluster as usize, run_start, run_end);
            if let Some(char_index) = self.char_index_for_byte(byte) {
                let x = position.x;
                match self.char_x.get_mut(char_index) {
                    Some(slot @ None) => *slot = Some(x),
                    Some(Some(existing)) if x < *existing => *existing = x,
                    _ => {}
                }
            }
        }

        self.current_point.x += info.advance.x;
        self.current_point.y += info.advance.y;
        self.advance_x = self.advance_x.max(self.current_point.x);
    }

    fn char_index_for_byte(&self, byte: usize) -> Option<usize> {
        if byte >= self.text.len() {
            return self.char_starts.len().checked_sub(2);
        }
        match self.char_starts.binary_search(&byte) {
            Ok(index) => Some(index.min(self.char_starts.len().saturating_sub(2))),
            Err(index) => index.checked_sub(1),
        }
    }
}

impl RunHandler for CspTextRunHandler<'_> {
    fn begin_line(&mut self) {
        self.current_point = Point::default();
    }

    fn run_info(&mut self, _info: &RunInfo) {}

    fn commit_run_info(&mut self) {}

    fn run_buffer(&mut self, info: &RunInfo) -> Buffer<'_> {
        self.pending_glyphs.resize(info.glyph_count, 0);
        self.pending_positions
            .resize(info.glyph_count, Point::default());
        self.pending_clusters.resize(info.glyph_count, 0);
        Buffer {
            glyphs: &mut self.pending_glyphs,
            positions: &mut self.pending_positions,
            offsets: None,
            clusters: Some(&mut self.pending_clusters),
            point: self.current_point,
        }
    }

    fn commit_run_buffer(&mut self, info: &RunInfo) {
        self.commit_pending_to_blob(info);
    }

    fn commit_line(&mut self) {}
}

fn normalize_cluster_byte(cluster: usize, run_start: usize, run_end: usize) -> usize {
    if cluster >= run_start && cluster <= run_end {
        cluster
    } else {
        run_start.saturating_add(cluster)
    }
}

fn dominant_script_tag(text: &str) -> FourByteTag {
    let mut han = 0usize;
    let mut kana = 0usize;
    let mut hangul = 0usize;
    let mut latin = 0usize;
    for ch in text.chars() {
        if is_han(ch) {
            han += 1;
        } else if is_kana(ch) {
            kana += 1;
        } else if is_hangul(ch) {
            hangul += 1;
        } else if ch.is_ascii_alphabetic() || ch.is_ascii_digit() {
            latin += 1;
        }
    }
    if kana > 0 && kana + han >= latin.max(hangul) {
        FourByteTag::from_chars('J', 'p', 'a', 'n')
    } else if han >= latin.max(hangul).max(1) {
        FourByteTag::from_chars('H', 'a', 'n', 's')
    } else if hangul >= latin.max(1) {
        FourByteTag::from_chars('H', 'a', 'n', 'g')
    } else if latin > 0 {
        FourByteTag::from_chars('L', 'a', 't', 'n')
    } else {
        FourByteTag::from_chars('Z', 'y', 'y', 'y')
    }
}

fn dominant_language_tag(text: &str) -> &'static str {
    let mut han = 0usize;
    let mut kana = 0usize;
    let mut hangul = 0usize;
    for ch in text.chars() {
        if is_han(ch) {
            han += 1;
        } else if is_kana(ch) {
            kana += 1;
        } else if is_hangul(ch) {
            hangul += 1;
        }
    }
    if kana > 0 && kana + han >= hangul {
        "ja-JP"
    } else if han > 0 {
        "zh-CN"
    } else if hangul > 0 {
        "ko-KR"
    } else {
        "en-US"
    }
}

fn is_han(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2ffff
    )
}

fn is_kana(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x30ff | 0x31f0..=0x31ff)
}

fn is_hangul(ch: char) -> bool {
    matches!(ch as u32, 0xac00..=0xd7af | 0x1100..=0x11ff | 0x3130..=0x318f)
}

#[cfg(test)]
mod tests {
    use skia_safe::{Font, FontMgr, FontStyle};

    use super::{
        ShapedTextLineRunInput, dominant_language_tag, dominant_script_tag, normalize_cluster_byte,
        shape_text_line,
    };

    #[test]
    fn cluster_bytes_normalize_relative_or_absolute_runs() {
        assert_eq!(normalize_cluster_byte(4, 10, 20), 14);
        assert_eq!(normalize_cluster_byte(12, 10, 20), 12);
    }

    #[test]
    fn dominant_tags_follow_text_script() {
        assert_eq!(*dominant_script_tag("test"), 0x4c61746e);
        assert_eq!(*dominant_script_tag("\u{6d4b}\u{8bd5}"), 0x48616e73);
        assert_eq!(dominant_language_tag("\u{30c6}\u{30b9}\u{30c8}"), "ja-JP");
    }

    #[test]
    fn shaped_line_plan_accumulates_run_origins_and_char_positions() {
        let typeface = FontMgr::new()
            .legacy_make_typeface(None, FontStyle::normal())
            .expect("default typeface");
        let font = Font::from_typeface(typeface, 24.0);

        let plan = shape_text_line(&[
            ShapedTextLineRunInput {
                text: "A",
                font: &font,
            },
            ShapedTextLineRunInput {
                text: "B",
                font: &font,
            },
        ])
        .expect("shaped line plan");

        assert_eq!(plan.runs.len(), 2);
        assert_eq!(plan.runs[0].input_index, 0);
        assert_eq!(plan.runs[0].x, 0.0);
        assert_eq!(plan.runs[1].input_index, 1);
        assert!(plan.runs[1].x > 0.0);
        assert_eq!(plan.char_positions.len(), 2);
        assert!(plan.char_positions[1].0 >= plan.char_positions[0].1);
        assert!(plan.advance_x >= plan.char_positions[1].1);
    }
}
