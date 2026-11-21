//! Reference-order numbering and definitions for footnotes and endnotes.

use std::collections::HashMap;

use super::review::paragraph_mark;
use super::Writer;
use crate::critic;
use crate::ooxml::Element;
use crate::ConvertError;

impl Writer<'_> {
    pub(super) fn reference_note(&mut self, is_endnote: bool, id: &str) -> usize {
        let key = (is_endnote, id.to_string());
        match self.note_refs.iter().position(|k| *k == key) {
            Some(i) => i + 1,
            None => {
                self.note_refs.push(key);
                self.note_changes.push(self.revisions.clone());
                self.note_refs.len()
            }
        }
    }

    pub(super) fn append_definitions(
        &mut self,
        markdown: &mut String,
        notes: &HashMap<(bool, String), Element>,
        output_limit: usize,
    ) -> Result<(), ConvertError> {
        // Footnotes and endnotes, numbered in reference order. A comment range the
        // body never closed does not run on into them.
        self.open_comments.clear();
        let mut index = 0;
        let mut has_definitions = false;
        while index < self.note_refs.len() {
            let key = self.note_refs[index].clone();
            // A note whose reference was inserted or deleted was inserted or
            // deleted with it, label and all.
            let changes = self.note_changes[index].clone();
            index += 1;
            let Some(note) = notes.get(&key) else {
                continue;
            };
            let around = std::mem::replace(&mut self.revisions, changes);
            self.lead = Some(format!("[^{index}]: "));
            // A first definition needs two separating newlines; subsequent ones
            // need one. The final document newline was reserved above. There is no
            // second output budget for notes, regardless of their source part.
            let separator = if has_definitions { "\n" } else { "\n\n" };
            let remaining = output_limit
                .saturating_sub(markdown.len())
                .saturating_sub(separator.len() + 1);
            let mut definition = critic::Marked::new(" ", remaining);
            for p in note.children_named("p") {
                let inline = self.paragraph_inline(p).0.into_inline();
                definition.push((
                    inline.render(true).replace('\n', " "),
                    paragraph_mark(p),
                    inline.edges(),
                ))?;
            }
            self.revisions = around;
            if let Some(error) = self.failure.take() {
                return Err(error);
            }
            // Still set: no paragraph had text, so there is no note.
            if self.lead.take().is_none() {
                let definition = definition.finish();
                if markdown
                    .len()
                    .saturating_add(separator.len())
                    .saturating_add(definition.len())
                    .saturating_add(1)
                    > output_limit
                {
                    return Err(ConvertError::limit(
                        "output",
                        crate::limits::MAX_OUTPUT_BYTES,
                    ));
                }
                markdown.push_str(separator);
                markdown.push_str(&definition);
                has_definitions = true;
            }
        }
        Ok(())
    }
}
