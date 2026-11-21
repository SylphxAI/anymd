//! Joins rendered paragraphs and cells while preserving tracked separators.

use super::{note, Change, Mark};

/// Appends `prefix` and `body` to `out` after `separator`. When the separator is
/// itself tracked (a paragraph mark that was inserted or deleted), it is marked
/// the way the CriticMarkup spec marks a paragraph break (`{++\n\n++}`), with
/// its attribution after it. An adjoining span of the same kind and attribution
/// is extended rather than opened twice.
///
/// `space` says the text on either side of a tracked break had a space there,
/// which rendering trimmed: once the break is accepted or rejected away the two
/// paragraphs join with that space, so it goes after the marked break.
pub(crate) fn splice(
    out: &mut String,
    separator: &str,
    prefix: &str,
    body: &str,
    change: Option<&Change>,
    space: bool,
) {
    let Some((mark, by)) = change else {
        out.push_str(separator);
        out.push_str(prefix);
        out.push_str(body);
        return;
    };
    let (open, close) = mark.delimiters();
    let by = by.as_deref().map(note).unwrap_or_default();
    let end = format!("{close}{by}");
    match out.strip_suffix(end.as_str()) {
        Some(kept) => out.truncate(kept.len()),
        None => out.push_str(open),
    }
    out.push_str(separator);
    out.push_str(prefix);
    // The body's first span continues this one when it is of the same kind and
    // its closer carries the same attribution. Document text is escaped and
    // converter-built Markdown is defused, so neither contains the closer, and a
    // span never nests one of its own kind: the first closer after the opener
    // is that span's own.
    let continues = body.strip_prefix(open).filter(|rest| {
        rest.find(close)
            .is_some_and(|at| rest[at + close.len()..].starts_with(by.as_str()))
    });
    match continues {
        Some(rest) => out.push_str(rest),
        None => {
            out.push_str(&end);
            if space {
                out.push(' ');
            }
            out.push_str(body);
        }
    }
}

/// Exact post-splice length, without building the combined Markdown. Keep the
/// same continuation rules as `splice`, including removed/merged delimiters.
pub(crate) fn splice_len(
    out: &str,
    separator: &str,
    prefix: &str,
    body: &str,
    change: Option<&Change>,
    space: bool,
) -> usize {
    let Some((mark, by)) = change else {
        return out
            .len()
            .saturating_add(separator.len())
            .saturating_add(prefix.len())
            .saturating_add(body.len());
    };
    let (open, close) = mark.delimiters();
    let by = by.as_deref().map(note).unwrap_or_default();
    let end = format!("{close}{by}");
    let start = out
        .strip_suffix(end.as_str())
        .map_or_else(|| out.len().saturating_add(open.len()), str::len);
    let continues = body.strip_prefix(open).filter(|rest| {
        rest.find(close)
            .is_some_and(|at| rest[at + close.len()..].starts_with(by.as_str()))
    });
    start
        .saturating_add(separator.len())
        .saturating_add(prefix.len())
        .saturating_add(continues.map_or_else(
            || {
                end.len()
                    .saturating_add(usize::from(space))
                    .saturating_add(body.len())
            },
            str::len,
        ))
}

/// The change a rendered part is wholly inside, with its attribution:
/// `{--gone--}{>>Ana<<}` is `(Deletion, Some("Ana"))`. Document text is
/// escaped, so the first closer after the opener is the span's own.
fn whole_change(part: &str) -> Option<Change> {
    [Mark::Insertion, Mark::Deletion]
        .into_iter()
        .find_map(|mark| {
            let (open, close) = mark.delimiters();
            let inner = part.strip_prefix(open)?;
            let after = &inner[inner.find(close)? + close.len()..];
            match after {
                "" => Some((mark, None)),
                _ => {
                    let by = after.strip_prefix("{>>")?.strip_suffix("<<}")?;
                    (!by.contains("<<}")).then(|| (mark, Some(by.to_string())))
                }
            }
        })
}

/// Joins parts that sit side by side, such as the cells of a flattened
/// table. A part that is wholly one change takes the separator before it into
/// that change (the first part, the separator after it), so accepting or
/// rejecting the change leaves no stray separator.
pub(crate) fn join_changed(parts: Vec<String>, separator: &str) -> String {
    let mut parts = parts.into_iter();
    let mut out = parts.next().unwrap_or_default();
    let first = whole_change(&out);
    for (index, part) in parts.enumerate() {
        let change = whole_change(&part).or_else(|| first.clone().filter(|_| index == 0));
        splice(&mut out, separator, "", &part, change.as_ref(), false);
    }
    out
}

/// A rendered paragraph, the tracked change on the mark that ends it, and
/// whether its text started and ended with a space before rendering trimmed it.
pub(crate) type Part = (String, Option<Change>, (bool, bool));

/// Joins paragraphs, each paired with the tracked change on the paragraph mark
/// that ends it. A blank paragraph drops the pending change: its own break stays.
pub(crate) fn join_marked(parts: Vec<Part>, separator: &str) -> String {
    let mut joined = Marked::new(separator, usize::MAX);
    for part in parts {
        // An unlimited join cannot refuse.
        joined.push(part).expect("unlimited paragraph join");
    }
    joined.finish()
}

/// Incremental paragraph join: refuses before extending its retained output.
/// DOCX definitions share their remaining allowance with the body, rather than
/// collecting an independently capped definitions vector and joining it later.
pub(crate) struct Marked<'a> {
    out: String,
    separator: &'a str,
    limit: usize,
    pending: Option<Change>,
    trailing: bool,
    first: Option<Change>,
}

impl<'a> Marked<'a> {
    pub(crate) fn new(separator: &'a str, limit: usize) -> Self {
        Self {
            out: String::new(),
            separator,
            limit,
            pending: None,
            trailing: false,
            first: None,
        }
    }

    pub(crate) fn push(
        &mut self,
        (part, mark, (leading, ends_in_space)): Part,
    ) -> Result<(), crate::ConvertError> {
        if part.trim().is_empty() {
            self.pending = None;
            return Ok(());
        }
        if self.out.is_empty() {
            if part.len() > self.limit {
                return Err(crate::ConvertError::limit(
                    "output",
                    crate::limits::MAX_OUTPUT_BYTES,
                ));
            }
            self.first = whole_change(&part);
            self.out = part;
        } else {
            let first = self.first.take();
            let change = self
                .pending
                .take()
                .or_else(|| whole_change(&part))
                .or(first);
            let space = self.trailing || leading;
            if splice_len(&self.out, self.separator, "", &part, change.as_ref(), space) > self.limit
            {
                return Err(crate::ConvertError::limit(
                    "output",
                    crate::limits::MAX_OUTPUT_BYTES,
                ));
            }
            splice(
                &mut self.out,
                self.separator,
                "",
                &part,
                change.as_ref(),
                space,
            );
        }
        self.pending = mark;
        self.trailing = ends_in_space;
        Ok(())
    }

    pub(crate) fn finish(self) -> String {
        self.out
    }
}
