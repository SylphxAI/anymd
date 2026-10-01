"""Build the original, public CC0 contract fixture; not a real-world QA corpus."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent / "fixtures"


def build():
    ROOT.mkdir(exist_ok=True)
    pages = [
        ("1. Launch", "The launch code is ORBIT.", "Launch review happens on Tuesday."),
        ("2. Recovery", "The recovery code is HARBOR.", "Recovery review happens on Friday."),
    ]
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>",
               b"<< /Type /Pages /Kids [4 0 R 6 0 R] /Count 2 >>",
               b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
    for index, lines in enumerate(pages):
        stream = "BT /F1 24 Tf 72 720 Td (" + lines[0] + ") Tj /F1 12 Tf"
        for line in lines[1:]:
            stream += " 0 -36 Td (" + line + ") Tj"
        stream = (stream + " ET").encode()
        objects.extend([
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {5 + index * 2} 0 R >>".encode(),
            b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
        ])
    pdf = b"%PDF-1.4\n"
    offsets = [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(pdf))
        pdf += f"{i} 0 obj\n".encode() + obj + b"\nendobj\n"
    xref = len(pdf)
    pdf += f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode()
    pdf += b"".join(f"{offset:010d} 00000 n \n".encode() for offset in offsets[1:])
    pdf += f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    (ROOT / "navigation.pdf").write_bytes(pdf)
    manifest = {
        "schema": 1, "license": "CC0-1.0", "track": "synthetic-contract-smoke",
        "documents": [{"id": "navigation", "file": "navigation.pdf", "sha256": hashlib.sha256(pdf).hexdigest(), "pages": 2}],
        "questions": [
            {"id": "launch", "document": "navigation", "question": "What is the launch code?", "query": "launch code", "section": "Launch", "pattern": "launch code is ([A-Z]+)", "answers": ["ORBIT"], "evidence_pages": [1]},
            {"id": "recovery", "document": "navigation", "question": "What is the recovery code?", "query": "recovery code", "section": "Recovery", "pattern": "recovery code is ([A-Z]+)", "answers": ["HARBOR"], "evidence_pages": [2]},
        ],
    }
    (ROOT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    build()
