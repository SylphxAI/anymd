use lopdf::{Document, ObjectId};

mod linearization;
mod load;

pub(crate) use linearization::detect_pdfjs_linearization;

pub(crate) struct ParsedPdf {
    pub document: Document,
    pub pages: Vec<(u32, ObjectId)>,
    pub encryption_facts: Option<EncryptionFacts>,
    /// pdf.js-compatible linearization detection over the exact source bytes.
    pub is_linearized: bool,
    #[allow(dead_code)]
    pub source_hash: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EncryptionFacts {
    pub permissions: Option<i64>,
    /// Security handler name from Encrypt.Filter (pdf.js EncryptFilterName).
    /// Captured before lopdf removes the Encrypt dictionary on empty-password decrypt.
    pub filter_name: Option<String>,
}

#[derive(Debug)]
#[allow(dead_code)]
enum PdfValue {
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
    Array(Vec<PdfValue>),
    Dict(std::collections::BTreeMap<Vec<u8>, PdfValue>),
}
