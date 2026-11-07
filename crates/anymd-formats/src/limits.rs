//! Per-conversion accounting, independent of the reader's display budget.

use std::io::Read;

use crate::ConvertError;

/// Cumulative Markdown bytes in one non-PDF conversion (256 MiB).
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024 * 1024;
/// Cumulative inflated container bytes, counting every read (512 MiB).
pub const MAX_INFLATED_BYTES: usize = 512 * 1024 * 1024;

pub(crate) struct Limits {
    output_limit: usize,
    inflated: usize,
    output: usize,
    // Optional metadata/media reads may ignore malformed data, but never a limit.
    failure: Option<ConvertError>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            output_limit: MAX_OUTPUT_BYTES,
            inflated: 0,
            output: 0,
            failure: None,
        }
    }
}

impl Limits {
    #[cfg(test)]
    pub(crate) fn with_output_limit(output_limit: usize) -> Self {
        Self {
            output_limit,
            ..Self::default()
        }
    }

    pub(crate) fn check(&self) -> Result<(), ConvertError> {
        self.failure.clone().map_or(Ok(()), Err)
    }

    fn charge(&mut self, bytes: usize, output: bool) -> Result<(), ConvertError> {
        self.check()?;
        let (used, limit, resource) = if output {
            (&mut self.output, self.output_limit, "output")
        } else {
            (&mut self.inflated, MAX_INFLATED_BYTES, "inflated input")
        };
        if bytes > limit.saturating_sub(*used) {
            let error = ConvertError::LimitExceeded { resource, limit };
            self.failure = Some(error.clone());
            return Err(error);
        }
        *used += bytes;
        Ok(())
    }

    pub(crate) fn inflated(&mut self, bytes: usize) -> Result<(), ConvertError> {
        self.charge(bytes, false)
    }

    pub(crate) fn output(&mut self, markdown: &str) -> Result<(), ConvertError> {
        self.charge(markdown.len(), true)
    }

    /// Read actual decompressor output in fixed chunks, not the declared size.
    /// Stop before appending bytes which would cross the cumulative ceiling.
    pub(crate) fn read(&mut self, mut reader: impl Read) -> Result<Vec<u8>, ConvertError> {
        self.check()?;
        let mut out = Vec::new();
        let mut chunk = [0; 16 * 1024];
        loop {
            let count = reader.read(&mut chunk).map_err(|error| {
                ConvertError::Invalid(format!("cannot inflate container entry: {error}"))
            })?;
            if count == 0 {
                break;
            }
            self.charge(count, false)?;
            out.extend_from_slice(&chunk[..count]);
        }
        Ok(out)
    }
}

/// Validate one unit before retaining it. Converters with multiple units also
/// account for their sum with `Limits::output`.
pub(crate) fn check_output(markdown: &str) -> Result<(), ConvertError> {
    Limits::default().output(markdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_boundaries_and_sticky_refusal() {
        let mut limits = Limits::default();
        limits.charge(MAX_INFLATED_BYTES - 1, false).unwrap();
        limits.charge(1, false).unwrap();
        let error = ConvertError::LimitExceeded {
            resource: "inflated input",
            limit: MAX_INFLATED_BYTES,
        };
        assert_eq!(limits.charge(1, false), Err(error.clone()));
        assert_eq!(limits.read(&b""[..]), Err(error.clone()));
        assert_eq!(limits.check(), Err(error));

        let mut limits = Limits::default();
        limits.charge(MAX_OUTPUT_BYTES - 1, true).unwrap();
        limits.output("a").unwrap();
        assert_eq!(
            limits.output("b"),
            Err(ConvertError::LimitExceeded {
                resource: "output",
                limit: MAX_OUTPUT_BYTES,
            })
        );
    }

    #[test]
    fn actual_reads_are_charged_again_and_do_not_trust_headers() {
        let mut limits = Limits::default();
        limits.charge(MAX_INFLATED_BYTES - 3, false).unwrap();
        assert_eq!(limits.read(&b"ab"[..]).unwrap(), b"ab");
        assert!(matches!(
            limits.read(&b"ab"[..]),
            Err(ConvertError::LimitExceeded {
                resource: "inflated input",
                ..
            })
        ));
    }
}
