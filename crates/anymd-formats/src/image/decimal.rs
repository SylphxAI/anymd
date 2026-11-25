use super::OcrWord;

/// Tesseract reads a typewritten decimal point as a comma about as often as
/// it reads it as a period. On a scanned page the two marks are different
/// shapes: a period is a dot that sits on the digit baseline, and a comma is
/// that dot with a tail that hangs below it.
///
/// `restore_decimal_commas` re-reads each comma inside a number from the
/// page image and keeps it only when ink continues past the baseline of the
/// digits beside it. A comma after a word, a thousands separator in text,
/// and every digit stay as Tesseract read them. Nothing is rewritten toward
/// a known value.
pub fn restore_decimal_commas(words: &mut [OcrWord], image: &[u8]) {
    if !words.iter().any(|word| word_has_numeric_comma(&word.text)) {
        return;
    }
    let Ok(image) = ::image::load_from_memory(image) else {
        return;
    };
    let gray = image.to_luma8();
    // A trailing comma between two numbers can separate a day from a
    // year. A faint tail is not enough evidence to rewrite that boundary.
    // Tesseract's TSV words arrive in line/reading order.
    for index in 0..words.len() {
        let word = &words[index];
        let number_boundary = word.text.ends_with(',')
            && words.get(index + 1).is_some_and(|next| {
                next.line == word.line
                    && !next.text.is_empty()
                    && next.text.bytes().all(|ch| ch.is_ascii_digit())
            });
        let word = &mut words[index];
        if number_boundary || !word_has_numeric_comma(&word.text) {
            continue;
        }
        if let Some(text) = decimals_from_ink(&gray, word) {
            word.text = text;
        }
    }
}

/// A comma Tesseract may have written for a decimal point.
///
/// Two shapes qualify. A single comma with one to three digits before it
/// and exactly three after it (`10,850`, `124,966`): a typed decimal with
/// three places, which is also how a thousands separator is printed, so
/// only the ink decides. And a trailing comma on a number of at most three
/// digits (`1,`), which is how a typed `1.` comes back. A comma after a
/// word, and a number with several groups (`1,000,000`), are left alone.
fn word_has_numeric_comma(text: &str) -> bool {
    let Some((before, after)) = text.split_once(',') else {
        return false;
    };
    (1..=3).contains(&before.len())
        && before.bytes().all(|ch| ch.is_ascii_digit())
        && (after.is_empty() || after.len() == 3)
        && after.bytes().all(|ch| ch.is_ascii_digit())
}

/// Replace a number's commas with periods where the mark does not hang
/// below the digits. Returns `None` when the word's ink cannot be read.
fn decimals_from_ink(image: &::image::GrayImage, word: &OcrWord) -> Option<String> {
    let glyphs = word_glyphs(image, word);
    if glyphs.len() < 2 {
        return None;
    }
    let tallest = glyphs.iter().map(|glyph| glyph.height).max()?;
    let digits: Vec<&InkGlyph> = glyphs
        .iter()
        .filter(|glyph| glyph.height * 10 >= tallest * 7)
        .collect();
    if digits.is_empty() {
        return None;
    }
    let baseline = digits.iter().map(|glyph| glyph.bottom).max()?;
    let chars: Vec<char> = word.text.chars().collect();
    let mut marks = glyphs
        .iter()
        .filter(|glyph| glyph.height * 10 < tallest * 6);
    let mut out = String::with_capacity(word.text.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch != ',' || !comma_between_digits(&chars, index) {
            out.push(*ch);
            continue;
        }
        let Some(mark) = marks.next() else {
            out.push(*ch);
            continue;
        };
        // A comma's tail crosses the digit baseline by about a quarter of
        // the x-height; a period ends on the baseline. The margin is half
        // of that, so a period that Tesseract's box placed a pixel low
        // still reads as a period, and a comma still reads as a comma.
        let hangs = mark.bottom > baseline + tallest / 8;
        out.push(if hangs { ',' } else { '.' });
    }
    Some(out)
}

fn comma_between_digits(chars: &[char], index: usize) -> bool {
    let before = chars[..index].iter().rev().find(|c| !c.is_whitespace());
    let after = chars[index + 1..].iter().find(|c| !c.is_whitespace());
    match (before, after) {
        (Some(left), Some(right)) => left.is_ascii_digit() && right.is_ascii_digit(),
        (Some(left), None) => left.is_ascii_digit(),
        _ => false,
    }
}

struct InkGlyph {
    bottom: u32,
    height: u32,
}

/// Ink blobs of one word, left to right. The crop runs below Tesseract's
/// box, because that box stops at the dot and clips a comma's tail.
fn word_glyphs(image: &::image::GrayImage, word: &OcrWord) -> Vec<InkGlyph> {
    let below = word.height / 3;
    let x0 = word.left;
    let y0 = word.top;
    let x1 = word.left.saturating_add(word.width).min(image.width());
    let y1 = word
        .top
        .saturating_add(word.height)
        .saturating_add(below)
        .min(image.height());
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let width = (x1 - x0) as usize;
    let mut columns = vec![0u32; width];
    for y in y0..y1 {
        for x in x0..x1 {
            if image.get_pixel(x, y).0[0] < 160 {
                columns[(x - x0) as usize] += 1;
            }
        }
    }
    let mut glyphs = Vec::new();
    let mut start: Option<usize> = None;
    for (index, count) in columns.iter().copied().chain([0]).enumerate() {
        if count > 2 && start.is_none() {
            start = Some(index);
            continue;
        }
        if count > 2 {
            continue;
        }
        let Some(from) = start.take() else {
            continue;
        };
        if index - from >= 2 {
            let mut top = y1;
            let mut bottom = y0;
            let mut ink = 0u32;
            for y in y0..y1 {
                for x in (x0 + from as u32)..(x0 + index as u32) {
                    if image.get_pixel(x, y).0[0] < 160 {
                        ink += 1;
                        top = top.min(y);
                        bottom = bottom.max(y);
                    }
                }
            }
            if ink >= 4 && bottom > top {
                glyphs.push(InkGlyph {
                    bottom,
                    height: bottom - top + 1,
                });
            }
        }
    }
    glyphs
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{decimals_from_ink, restore_decimal_commas, word_has_numeric_comma, OcrWord};
    use crate::image::tests::png;

    /// Black ink for `ink`, white everywhere else.
    fn strip(width: u32, height: u32, ink: &[(u32, u32, u32, u32)]) -> ::image::GrayImage {
        let mut image = ::image::GrayImage::from_pixel(width, height, ::image::Luma([255]));
        for &(x0, y0, x1, y1) in ink {
            for y in y0..y1 {
                for x in x0..x1 {
                    image.put_pixel(x, y, ::image::Luma([0]));
                }
            }
        }
        image
    }

    fn boxed(text: &str, width: u32, height: u32) -> OcrWord {
        OcrWord {
            left: 0,
            top: 0,
            width,
            height,
            line: (1, 1, 1),
            confidence: 90.0,
            text: text.into(),
        }
    }

    #[test]
    fn a_mark_on_the_baseline_is_a_decimal_and_one_below_it_is_a_comma() {
        // Digits occupy y 4..36 (baseline 35). A dot ending at 37 stays a
        // period; a mark ending 9px past the baseline stays a comma.
        let period = strip(
            120,
            60,
            &[(4, 4, 20, 36), (28, 26, 40, 38), (48, 4, 70, 36)],
        );
        let comma = strip(
            120,
            60,
            &[(4, 4, 20, 36), (28, 26, 42, 48), (50, 4, 72, 36)],
        );
        let mut dot = boxed("124,966", 120, 40);
        assert_eq!(decimals_from_ink(&period, &dot).as_deref(), Some("124.966"));
        assert_eq!(decimals_from_ink(&comma, &dot).as_deref(), Some("124,966"));

        // "14," is a date: its comma hangs below the digits, so it is kept.
        // A comma after a word, and a number with two groups, is not a candidate.
        let date = strip(80, 60, &[(4, 4, 20, 36), (28, 4, 50, 36), (58, 28, 72, 48)]);
        dot = boxed("14,", 80, 40);
        assert_eq!(decimals_from_ink(&date, &dot).as_deref(), Some("14,"));
        assert!(!word_has_numeric_comma("research,"));
        assert!(!word_has_numeric_comma("1,000,000"));
        assert!(word_has_numeric_comma("14,"));
        assert!(word_has_numeric_comma("1,000"));
        assert!(word_has_numeric_comma("1,"));
        for text in ["1.2,345", "1,000.50", "1,2", ",123", "1234,567", "1,,000"] {
            assert!(!word_has_numeric_comma(text), "{text}");
        }
    }

    #[test]
    fn a_trailing_comma_before_another_number_is_not_a_decimal() {
        // Even when a faint comma's tail looks like a dot, the boundary
        // between a day and a separate year must remain unchanged.
        let image = strip(
            120,
            60,
            &[(4, 4, 20, 36), (28, 4, 50, 36), (58, 26, 72, 38)],
        );
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ::image::ImageFormat::Png).unwrap();
        let bytes = out.into_inner();
        assert_eq!(
            decimals_from_ink(&image, &boxed("14,", 120, 40)).as_deref(),
            Some("14.")
        );
        let mut words = vec![boxed("14,", 120, 40), boxed("1957", 120, 40)];
        restore_decimal_commas(&mut words, &bytes);
        assert_eq!(words[0].text, "14,");

        // A word on the next line cannot make this a numeric boundary.
        words[1].line.2 += 1;
        restore_decimal_commas(&mut words, &bytes);
        assert_eq!(words[0].text, "14.");

        // A list marker followed by a label can still be read from ink.
        words = vec![boxed("3,", 120, 40), boxed("Keep", 120, 40)];
        restore_decimal_commas(&mut words, &bytes);
        assert_eq!(words[0].text, "3.");
    }

    #[test]
    fn decimal_recovery_leaves_unreadable_images_and_boxes_unchanged() {
        let mut words = vec![boxed("124,966", 120, 40)];
        let original = words.clone();
        restore_decimal_commas(&mut words, b"not an image");
        assert_eq!(words, original);
        let image = png(1, 1);
        words[0].left = u32::MAX;
        words[0].top = u32::MAX;
        let original = words.clone();
        restore_decimal_commas(&mut words, &image);
        assert_eq!(words, original);
    }
}
