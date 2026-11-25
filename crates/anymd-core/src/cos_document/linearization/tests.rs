use super::detect_pdfjs_linearization;

fn wrap_first_object(dict: &str) -> Vec<u8> {
    let make = |l_value: &str| -> Vec<u8> {
        let dict_body = dict.replace("/L 0", &format!("/L {l_value}"));
        let mut parts: Vec<u8> = b"%PDF-1.4\n".to_vec();
        parts.extend_from_slice(&[b'%', 0xe2, 0xe3, 0xcf, 0xd3, b'\n']);
        let obj1 = format!("1 0 obj\n{dict_body}\nendobj\n");
        let obj2 = "2 0 obj\n<< /Type /Catalog /Pages 3 0 R >>\nendobj\n";
        let obj3 = "3 0 obj\n<< /Type /Pages /Kids [4 0 R] /Count 1 >>\nendobj\n";
        let obj4 = "4 0 obj\n<< /Type /Page /Parent 3 0 R /MediaBox [0 0 612 792] /Contents 5 0 R >>\nendobj\n";
        let obj5 = "5 0 obj\n<< /Length 20 >>\nstream\nBT /F1 1 Tf ET\nendstream\nendobj\n";
        let mut offsets = [0usize; 6];
        offsets[1] = parts.len();
        parts.extend_from_slice(obj1.as_bytes());
        offsets[2] = parts.len();
        parts.extend_from_slice(obj2.as_bytes());
        offsets[3] = parts.len();
        parts.extend_from_slice(obj3.as_bytes());
        offsets[4] = parts.len();
        parts.extend_from_slice(obj4.as_bytes());
        offsets[5] = parts.len();
        parts.extend_from_slice(obj5.as_bytes());
        let xref_pos = parts.len();
        let mut xref = String::from("xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            xref.push_str(&format!("{offset:010} 00000 n \n"));
        }
        parts.extend_from_slice(xref.as_bytes());
        parts.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 2 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n")
                .as_bytes(),
        );
        parts
    };

    if dict.contains("/L 0") {
        let mut bytes = make("1");
        for _ in 0..8 {
            let desired = bytes.len().to_string();
            bytes = make(&desired);
            if bytes.len().to_string() == desired {
                return bytes;
            }
        }
        return bytes;
    }
    make("0")
}

#[test]
fn valid_linearization_dict_is_detected() {
    let bytes = wrap_first_object("<< /Linearized 1 /L 0 /H [100 200] /O 4 /E 50 /N 1 /T 900 >>");
    assert!(
        detect_pdfjs_linearization(&bytes),
        "bytes len {} head {}",
        bytes.len(),
        String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
    );
}

#[test]
fn mismatched_l_is_not_linearized() {
    let bytes =
        wrap_first_object("<< /Linearized 1 /L 999999 /H [100 200] /O 4 /E 50 /N 1 /T 900 >>");
    assert!(!detect_pdfjs_linearization(&bytes));
}

#[test]
fn zero_hint_is_not_linearized() {
    let bytes = wrap_first_object("<< /Linearized 1 /L 0 /H [0 0] /O 4 /E 50 /N 1 /T 900 >>");
    assert!(!detect_pdfjs_linearization(&bytes));
}

#[test]
fn plain_first_object_is_not_linearized() {
    let bytes = wrap_first_object("<< /Type /Catalog /Pages 3 0 R >>");
    assert!(!detect_pdfjs_linearization(&bytes));
}
