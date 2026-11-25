use std::cell::RefCell;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::rc::Rc;

use crate::limits::Limits;
use crate::ooxml::EntryInflation;

/// Reserve the measured inflation of an entry before the parser starts reading
/// its compressed payload. Each reopen reserves it again. This covers all ZIP
/// formats and initialization attempts without duplicating calamine's parsers.
/// A partial read still reserves the whole entry, never just its compressed size.
#[derive(Clone)]
pub(super) struct AccountedInput<'a> {
    cursor: Cursor<&'a [u8]>,
    entries: Rc<Vec<EntryInflation>>,
    limits: Rc<RefCell<Limits>>,
    active: Option<usize>,
}

impl<'a> AccountedInput<'a> {
    pub(super) fn new(
        bytes: &'a [u8],
        entries: Vec<EntryInflation>,
        limits: Rc<RefCell<Limits>>,
    ) -> Self {
        Self {
            cursor: Cursor::new(bytes),
            entries: Rc::new(entries),
            limits,
            active: None,
        }
    }
}

impl Read for AccountedInput<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut limits = self.limits.borrow_mut();
        limits.check().map_err(std::io::Error::other)?;
        if buf.is_empty() {
            return Ok(0);
        }
        let position = self.cursor.position();
        let next = self
            .entries
            .partition_point(|entry| entry.start <= position);
        let entry = next
            .checked_sub(1)
            .map(|index| &self.entries[index])
            .filter(|entry| position < entry.end);
        let boundary = if let Some(entry) = entry {
            if self.active != Some(entry.index) {
                limits
                    .inflated(entry.inflated)
                    .map_err(std::io::Error::other)?;
                self.active = Some(entry.index);
            }
            Some(entry.end)
        } else {
            self.active = None;
            self.entries.get(next).map(|entry| entry.start)
        };
        // Do not let a read that starts in a header cross into an uncharged
        // payload, or let one entry's read consume the following entry.
        let count = boundary.map_or(buf.len(), |end| {
            buf.len()
                .min(usize::try_from(end - position).unwrap_or(usize::MAX))
        });
        self.cursor.read(&mut buf[..count])
    }
}

impl Seek for AccountedInput<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let after = self.cursor.seek(position)?;
        self.active = None;
        Ok(after)
    }
}
