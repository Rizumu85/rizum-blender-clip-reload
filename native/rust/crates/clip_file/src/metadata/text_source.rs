use std::collections::HashMap;

use clip_model::{LayerId, LayerKind, LayerVisibility, Rgba8};

use crate::ClipFileError;

use super::records::{
    LayerRecord, TextLayerAttributes, TextLayerEntry, TextLayerFontMapping, TextLayerRect,
    TextLayerRun, TextLayerSource, TextLayerSpan,
};
use super::schema::{checked_i64_to_u32, connect_sqlite, optional_text_expr, table_columns};

pub fn read_text_layer_source_from_sqlite(
    sqlite_bytes: &[u8],
    layer_id: LayerId,
) -> Result<TextLayerSource, ClipFileError> {
    let sources = read_text_layer_sources_from_sqlite(sqlite_bytes, &[layer_id])?;
    sources
        .get(&layer_id)
        .cloned()
        .ok_or(ClipFileError::MissingLayer(layer_id))
}

pub fn read_text_layer_sources_from_sqlite(
    sqlite_bytes: &[u8],
    layer_ids: &[LayerId],
) -> Result<HashMap<LayerId, TextLayerSource>, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let resolution_dpi = conn
        .query_row("SELECT CanvasResolution FROM Canvas LIMIT 1", [], |row| {
            row.get::<_, f64>(0)
        })
        .ok()
        .and_then(|dpi| {
            if dpi.is_finite() && dpi > 0.0 && dpi <= f64::from(u32::MAX) {
                Some(dpi.round() as u32)
            } else {
                None
            }
        })
        .unwrap_or(72);
    let query = format!(
        "SELECT MainId, {}, LayerType, LayerVisibility, {}, {}, {}, {} \
         FROM Layer WHERE MainId = ?",
        optional_text_expr(&layer_columns, "LayerName"),
        optional_blob_expr(&layer_columns, "TextLayerString"),
        optional_blob_expr(&layer_columns, "TextLayerAttributes"),
        optional_blob_expr(&layer_columns, "TextLayerStringArray"),
        optional_blob_expr(&layer_columns, "TextLayerAttributesArray"),
    );
    let mut stmt = conn.prepare(&query)?;
    let mut sources = HashMap::new();
    for layer_id in layer_ids {
        let source = match stmt.query_row([layer_id.0], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let _layer_type: i64 = row.get(2)?;
            let visibility: i64 = row.get(3)?;
            let string: Option<Vec<u8>> = row.get(4)?;
            let attributes: Option<Vec<u8>> = row.get(5)?;
            let string_array: Option<Vec<u8>> = row.get(6)?;
            let attributes_array: Option<Vec<u8>> = row.get(7)?;
            Ok((
                id,
                name,
                visibility,
                string,
                attributes,
                string_array,
                attributes_array,
            ))
        }) {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => continue,
            Err(err) => return Err(err.into()),
        };
        let (id, name, visibility, string, attributes, string_array, attributes_array) = source;
        let id = LayerId(checked_i64_to_u32(id, "Layer.MainId")?);
        let strings = text_blob_array(string, string_array)?;
        let attributes = blob_array(attributes, attributes_array)?;
        let entries = strings
            .into_iter()
            .zip(attributes.into_iter())
            .map(|(text, attributes)| {
                Ok(TextLayerEntry {
                    text,
                    attributes: parse_text_layer_attributes(&attributes)?,
                })
            })
            .collect::<Result<Vec<_>, ClipFileError>>()?;
        sources.insert(
            id,
            TextLayerSource {
                layer: LayerRecord {
                    id,
                    kind: LayerKind::Text,
                    visibility: LayerVisibility(checked_i64_to_u32(
                        visibility,
                        "Layer.LayerVisibility",
                    )?),
                },
                entries,
                resolution_dpi,
            },
        );
        let _ = name;
    }
    Ok(sources)
}

pub fn parse_text_layer_attributes(data: &[u8]) -> Result<TextLayerAttributes, ClipFileError> {
    let mut reader = DataReader::new(data);
    let mut attributes = TextLayerAttributes {
        default_font: None,
        fallback_font: None,
        fonts: Vec::new(),
        layout_flags: None,
        path_mode: None,
        path_angle_a_degrees: None,
        path_angle_b_degrees: None,
        path_center: None,
        font_size_100: None,
        color: None,
        bbox: None,
        quad_verts_100: None,
        box_size: None,
        align: None,
        underline_spans: Vec::new(),
        strikethrough_spans: Vec::new(),
        runs: Vec::new(),
    };
    while reader.left() >= 8 {
        let param_id = reader.read_u32_le()?;
        let size = reader.read_u32_le()? as usize;
        let payload = reader.read_bytes(size)?;
        parse_text_param(param_id, payload, &mut attributes)?;
    }
    Ok(attributes)
}

fn parse_text_param(
    param_id: u32,
    payload: &[u8],
    attributes: &mut TextLayerAttributes,
) -> Result<(), ClipFileError> {
    let mut reader = DataReader::new(payload);
    match param_id {
        11 => {
            let count = reader.read_u32_le()?;
            for _ in 0..count {
                let start = reader.read_i32_le()?;
                let length = reader.read_u32_le()?;
                let entry_size = reader.read_u32_le()? as usize;
                let entry_start = reader.position();
                let entry_payload_len = entry_size
                    .checked_sub(8)
                    .ok_or(ClipFileError::InvalidMetadata("TextLayerAttributes.runs"))?;
                let entry_end = entry_start
                    .checked_add(entry_payload_len)
                    .ok_or(ClipFileError::TileSizeOverflow)?;
                if entry_end > payload.len() {
                    return Err(ClipFileError::InvalidMetadata("TextLayerAttributes.runs"));
                }
                let style_flags = reader.read_u8()?;
                let field_defaults_flags = reader.read_u8()?;
                let color = Rgba8 {
                    r: u16_to_u8(reader.read_u16_le()?),
                    g: u16_to_u8(reader.read_u16_le()?),
                    b: u16_to_u8(reader.read_u16_le()?),
                    a: 255,
                };
                let font_scale = reader.read_f64_le()?.round() as i32;
                let font_len = usize::from(reader.read_u16_le()?);
                let font = if font_len == 0 {
                    None
                } else {
                    Some(reader.read_utf16le_string(font_len)?)
                };
                attributes.runs.push(TextLayerRun {
                    start,
                    length,
                    style_flags,
                    field_defaults_flags,
                    color,
                    font_scale,
                    font,
                });
                reader.set_position(entry_end)?;
            }
        }
        12 => {
            let count = reader.read_u32_le()?;
            let mut totals: HashMap<u8, u32> = HashMap::new();
            for _ in 0..count {
                let _start = reader.read_i32_le()?;
                let length = reader.read_u32_le()?;
                let _unknown = reader.read_u32_le()?;
                let align = reader.read_u8()?;
                let _unknown2 = reader.read_u8()?;
                *totals.entry(align).or_default() += length;
            }
            attributes.align = totals
                .into_iter()
                .max_by_key(|(align, total)| (*total, std::cmp::Reverse(*align)))
                .map(|(align, _)| align);
        }
        16 => {
            attributes.underline_spans = parse_text_span_param(payload)?;
        }
        20 => {
            attributes.strikethrough_spans = parse_text_span_param(payload)?;
        }
        26 => {
            if payload.len() >= 24 {
                attributes.box_size =
                    Some((reader_at_i32(payload, 0)?, reader_at_i32(payload, 8)?));
            }
        }
        31 => {
            if !payload.is_empty() {
                attributes.default_font = Some(read_utf8_lossless(payload)?);
            }
        }
        32 => {
            attributes.font_size_100 = Some(reader.read_i32_le()?);
        }
        33 => {
            attributes.layout_flags = Some(reader.read_i32_le()?);
        }
        34 => {
            attributes.color = Some(Rgba8 {
                r: u32_to_u8(reader.read_u32_le()?),
                g: u32_to_u8(reader.read_u32_le()?),
                b: u32_to_u8(reader.read_u32_le()?),
                a: 255,
            });
        }
        42 => {
            attributes.bbox = Some(TextLayerRect {
                left: reader.read_i32_le()?,
                top: reader.read_i32_le()?,
                right: reader.read_i32_le()?,
                bottom: reader.read_i32_le()?,
            });
        }
        47 => {
            attributes.fallback_font = parse_param47_font(payload);
        }
        57 => {
            let count = reader.read_u16_le()?;
            let mut fonts = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                let display_len = usize::from(reader.read_u16_le()?);
                let display_name = reader.read_utf8_string(display_len)?;
                let font_len = usize::from(reader.read_u16_le()?);
                let font_name = reader.read_utf8_string(font_len)?;
                fonts.push(TextLayerFontMapping {
                    display_name,
                    font_name,
                });
            }
            attributes.fonts = fonts;
        }
        63 => {
            attributes.box_size = Some((reader.read_i32_le()?, reader.read_i32_le()?));
        }
        64 => {
            let mut verts = [0i32; 8];
            for value in &mut verts {
                *value = reader.read_i32_le()?;
            }
            attributes.quad_verts_100 = Some(verts);
        }
        66 => {
            attributes.path_mode = Some(reader.read_i32_le()?);
        }
        70 => {
            attributes.path_angle_a_degrees = Some(reader.read_f64_be()?.round() as i32);
        }
        71 => {
            attributes.path_angle_b_degrees = Some(reader.read_f64_be()?.round() as i32);
        }
        72 => {
            attributes.path_center = Some((reader.read_i32_le()?, reader.read_i32_le()?));
        }
        _ => {}
    }
    Ok(())
}

fn parse_text_span_param(payload: &[u8]) -> Result<Vec<TextLayerSpan>, ClipFileError> {
    if payload.is_empty() {
        return Ok(Vec::new());
    }
    let mut reader = DataReader::new(payload);
    let count = reader.read_u32_le()?;
    let mut spans = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let start = reader.read_i32_le()?;
        let length = reader.read_u32_le()?;
        let _unknown = reader.read_u32_le()?;
        let enabled = reader.read_u8()? != 0;
        let _unknown2 = reader.read_u8()?;
        if enabled {
            spans.push(TextLayerSpan { start, length });
        }
    }
    Ok(spans)
}

fn optional_blob_expr(columns: &std::collections::HashSet<String>, column: &str) -> String {
    if columns.contains(column) {
        column.to_owned()
    } else {
        format!("NULL AS {column}")
    }
}

fn text_blob_array(
    first: Option<Vec<u8>>,
    array: Option<Vec<u8>>,
) -> Result<Vec<String>, ClipFileError> {
    blob_array(first, array)?
        .into_iter()
        .map(|blob| {
            String::from_utf8(blob).map_err(|_| ClipFileError::InvalidMetadata("TextLayerString"))
        })
        .collect()
}

fn blob_array(
    first: Option<Vec<u8>>,
    array: Option<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, ClipFileError> {
    let mut values = Vec::new();
    if let Some(first) = first {
        values.push(first);
    }
    if let Some(array) = array {
        let mut reader = DataReader::new(&array);
        while reader.left() >= 4 {
            let size = reader.read_u32_le()? as usize;
            values.push(reader.read_bytes(size)?.to_vec());
        }
        if reader.left() != 0 {
            return Err(ClipFileError::InvalidMetadata("TextLayerArray"));
        }
    }
    Ok(values)
}

fn parse_param47_font(payload: &[u8]) -> Option<String> {
    if payload.len() < 12 {
        return None;
    }
    let len = u16::from_le_bytes(payload.get(10..12)?.try_into().ok()?) as usize;
    let end = 12usize.checked_add(len)?;
    let bytes = payload.get(12..end)?;
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

fn read_utf8_lossless(bytes: &[u8]) -> Result<String, ClipFileError> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| ClipFileError::InvalidMetadata("TextLayerAttributes.utf8"))
}

fn reader_at_i32(bytes: &[u8], offset: usize) -> Result<i32, ClipFileError> {
    let end = offset
        .checked_add(4)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let raw = bytes
        .get(offset..end)
        .ok_or(ClipFileError::InvalidMetadata("TextLayerAttributes.i32"))?;
    Ok(i32::from_le_bytes(raw.try_into().unwrap()))
}

fn u16_to_u8(value: u16) -> u8 {
    ((u32::from(value) * 255 + 32767) / 65535) as u8
}

fn u32_to_u8(value: u32) -> u8 {
    ((u64::from(value) * 255 + 2_147_483_647) / 4_294_967_295) as u8
}

struct DataReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> DataReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn position(&self) -> usize {
        self.pos
    }

    fn set_position(&mut self, pos: usize) -> Result<(), ClipFileError> {
        if pos > self.data.len() {
            return Err(ClipFileError::InvalidMetadata("TextLayerAttributes.offset"));
        }
        self.pos = pos;
        Ok(())
    }

    fn left(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], ClipFileError> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        let bytes = self
            .data
            .get(self.pos..end)
            .ok_or(ClipFileError::InvalidMetadata("TextLayerAttributes.bytes"))?;
        self.pos = end;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8, ClipFileError> {
        Ok(*self
            .read_bytes(1)?
            .first()
            .ok_or(ClipFileError::InvalidMetadata("TextLayerAttributes.u8"))?)
    }

    fn read_u16_le(&mut self) -> Result<u16, ClipFileError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u32_le(&mut self) -> Result<u32, ClipFileError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i32_le(&mut self) -> Result<i32, ClipFileError> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_f64_le(&mut self) -> Result<f64, ClipFileError> {
        let bytes = self.read_bytes(8)?;
        Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_f64_be(&mut self) -> Result<f64, ClipFileError> {
        let bytes = self.read_bytes(8)?;
        Ok(f64::from_be_bytes(bytes.try_into().unwrap()))
    }

    fn read_utf8_string(&mut self, len: usize) -> Result<String, ClipFileError> {
        read_utf8_lossless(self.read_bytes(len)?)
    }

    fn read_utf16le_string(&mut self, len: usize) -> Result<String, ClipFileError> {
        let bytes = self.read_bytes(len.checked_mul(2).ok_or(ClipFileError::TileSizeOverflow)?)?;
        let units = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|_| ClipFileError::InvalidMetadata("TextLayerAttributes.utf16"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_text_attributes() {
        let mut data = Vec::new();
        push_param(&mut data, 31, b"HarmonyOS Sans Bold");
        push_param(&mut data, 32, &900i32.to_le_bytes());
        push_param(&mut data, 33, &16i32.to_le_bytes());
        push_param(&mut data, 66, &1i32.to_le_bytes());
        push_param(&mut data, 70, &195f64.to_be_bytes());
        push_param(&mut data, 71, &165f64.to_be_bytes());
        let mut path_center = Vec::new();
        path_center.extend_from_slice(&171i32.to_le_bytes());
        path_center.extend_from_slice(&171i32.to_le_bytes());
        push_param(&mut data, 72, &path_center);
        let mut color = Vec::new();
        color.extend_from_slice(&0x27272727u32.to_le_bytes());
        color.extend_from_slice(&0x27272727u32.to_le_bytes());
        color.extend_from_slice(&0x27272727u32.to_le_bytes());
        push_param(&mut data, 34, &color);
        let mut bbox = Vec::new();
        for value in [20i32, 37, 242, 145] {
            bbox.extend_from_slice(&value.to_le_bytes());
        }
        push_param(&mut data, 42, &bbox);

        let parsed = parse_text_layer_attributes(&data).unwrap();

        assert_eq!(parsed.default_font.as_deref(), Some("HarmonyOS Sans Bold"));
        assert_eq!(parsed.font_size_100, Some(900));
        assert_eq!(parsed.layout_flags, Some(16));
        assert_eq!(parsed.path_mode, Some(1));
        assert_eq!(parsed.path_angle_a_degrees, Some(195));
        assert_eq!(parsed.path_angle_b_degrees, Some(165));
        assert_eq!(parsed.path_center, Some((171, 171)));
        assert_eq!(
            parsed.color,
            Some(Rgba8 {
                r: 39,
                g: 39,
                b: 39,
                a: 255
            })
        );
        assert_eq!(
            parsed.bbox,
            Some(TextLayerRect {
                left: 20,
                top: 37,
                right: 242,
                bottom: 145
            })
        );
    }

    #[test]
    fn parses_text_decoration_spans() {
        let mut data = Vec::new();
        push_param(&mut data, 16, &span_payload(1));
        push_param(&mut data, 20, &span_payload(1));

        let parsed = parse_text_layer_attributes(&data).unwrap();

        assert_eq!(
            parsed.underline_spans,
            vec![TextLayerSpan {
                start: 0,
                length: 4
            }]
        );
        assert_eq!(
            parsed.strikethrough_spans,
            vec![TextLayerSpan {
                start: 0,
                length: 4
            }]
        );
    }

    fn span_payload(enabled: u8) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&0i32.to_le_bytes());
        payload.extend_from_slice(&4u32.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.push(enabled);
        payload.push(0);
        payload
    }

    fn push_param(data: &mut Vec<u8>, id: u32, payload: &[u8]) {
        data.extend_from_slice(&id.to_le_bytes());
        data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        data.extend_from_slice(payload);
    }
}
