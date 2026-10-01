//! Deterministic visual lines shared by the inline editor, page overlay and PDF appearance.
//! Explicit newlines stay in the annotation value; soft wraps affect drawing only.
use std::sync::OnceLock;
use ttf_parser::Face;

pub const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");
static FACE: OnceLock<Option<Face<'static>>> = OnceLock::new();

pub fn font_face() -> Option<&'static Face<'static>> {
    FACE.get_or_init(|| Face::parse(FONT_BYTES, 0).ok())
        .as_ref()
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualLine<'a> {
    pub text: &'a str,
    pub source_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub width: f64,
}

pub fn text_width(face: &Face<'_>, text: &str, size: f64) -> f64 {
    text.chars()
        .filter_map(|ch| {
            let glyph = face.glyph_index(ch)?;
            face.glyph_hor_advance(glyph)
        })
        .map(|advance| f64::from(advance) * size / f64::from(face.units_per_em()))
        .sum()
}

pub fn wrap_text<'a>(
    value: &'a str,
    max_width: f64,
    size: f64,
    face: &Face<'_>,
) -> Vec<VisualLine<'a>> {
    let mut lines = Vec::new();
    for (source_line, paragraph) in value.split('\n').enumerate() {
        if paragraph.is_empty() || !max_width.is_finite() || max_width <= 0.0 {
            lines.push(VisualLine {
                text: paragraph,
                source_line,
                start_byte: 0,
                end_byte: paragraph.len(),
                width: text_width(face, paragraph, size),
            });
            continue;
        }
        let mut start = 0;
        while start < paragraph.len() {
            let mut width = 0.0;
            let mut end = start;
            let mut word_break = None;
            for (relative, ch) in paragraph[start..].char_indices() {
                let next = start + relative + ch.len_utf8();
                let advance = text_width(face, &paragraph[start + relative..next], size);
                if width + advance > max_width && end > start {
                    break;
                }
                width += advance;
                end = next;
                if ch == ' ' {
                    word_break = Some(start + relative);
                }
            }
            let (visible_end, logical_end) = if end == paragraph.len() {
                (
                    start + paragraph[start..end].trim_end_matches(' ').len(),
                    end,
                )
            } else if let Some(space_start) = word_break {
                let visible_end = start + paragraph[start..space_start].trim_end_matches(' ').len();
                let logical_end = space_start
                    + paragraph[space_start..]
                        .chars()
                        .take_while(|ch| *ch == ' ')
                        .count();
                (visible_end, logical_end)
            } else if paragraph[end..].starts_with(' ') {
                let logical_end =
                    end + paragraph[end..].chars().take_while(|ch| *ch == ' ').count();
                (end, logical_end)
            } else {
                (end, end)
            };
            if visible_end > start || logical_end == paragraph.len() {
                let text = &paragraph[start..visible_end];
                lines.push(VisualLine {
                    text,
                    source_line,
                    start_byte: start,
                    end_byte: logical_end,
                    width: text_width(face, text, size),
                });
            }
            start = logical_end;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words_and_long_unicode_tokens_without_changing_source() {
        let face = font_face().unwrap();
        let value = "Café Ω longword\n\nЖЖЖЖ";
        let width = text_width(face, "Café Ω ", 16.0).max(text_width(face, "longword", 16.0)) + 0.1;
        let lines = wrap_text(value, width, 16.0, face);
        assert_eq!(lines[0].text, "Café Ω");
        assert_eq!(lines[0].end_byte, "Café Ω ".len());
        assert_eq!(lines[1].text, "longword");
        assert_eq!(lines[2].text, "");
        assert_eq!(lines[3].text, "ЖЖЖЖ");
        assert_eq!(lines[1].source_line, 0);
        assert_eq!(lines[1].start_byte, "Café Ω ".len());
        assert_eq!(lines[3].source_line, 2);

        let tiny = text_width(face, "Ж", 16.0) + 0.1;
        let glyph_lines = wrap_text("ЖЖЖ", tiny, 16.0, face);
        assert_eq!(
            glyph_lines.iter().map(|line| line.text).collect::<Vec<_>>(),
            vec!["Ж", "Ж", "Ж"]
        );
        assert_eq!(glyph_lines.last().unwrap().end_byte, "ЖЖЖ".len());

        let narrow = wrap_text(
            "Café Ω longword voorbeeld en nog meerX tekst",
            130.0,
            32.0,
            face,
        );
        assert!(
            narrow.iter().all(|line| !line.text.trim().is_empty()),
            "{narrow:?}"
        );
    }
}
