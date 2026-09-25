use crate::{ConvertError, Converted, Options};

/// True when the leading bytes identify this media kind.
pub fn sniff(_head: &[u8]) -> bool {
    false
}

pub fn convert(_bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
    Err(ConvertError::Unsupported("video conversion is not implemented yet".into()))
}
