use crate::{ConvertError, Converted, Options};

pub fn convert(_bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
    Err(ConvertError::Unsupported("epub conversion is not implemented yet".into()))
}
