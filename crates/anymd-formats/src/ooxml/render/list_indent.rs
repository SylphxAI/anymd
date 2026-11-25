/// Tracks list marker widths so nested items indent under their parent's content.
#[derive(Default)]
pub(crate) struct ListIndent {
    widths: Vec<usize>,
}

impl ListIndent {
    pub(crate) fn reset(&mut self) {
        self.widths.clear();
    }

    /// Format an item at `level` with `marker` (e.g. "-", "3.").
    pub(crate) fn item(&mut self, level: usize, marker: &str, text: &str) -> String {
        let level = level.min(8);
        self.widths.truncate(level);
        while self.widths.len() < level {
            self.widths.push(2);
        }
        let indent = " ".repeat(self.widths.iter().sum());
        self.widths.push(marker.chars().count() + 1);
        let continuation = format!("\n{}", " ".repeat(self.widths.iter().sum()));
        format!("{indent}{marker} {}", text.replace('\n', &continuation))
    }
}
