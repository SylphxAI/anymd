"""Create the task guide's one-page native-text PDF. Python 3, no packages."""
from pathlib import Path

stream = b"BT /F1 12 Tf 50 700 Td (Deterministic quote) Tj ET"
objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
]
output = bytearray(b"%PDF-1.4\n")
offsets = []
for number, obj in enumerate(objects, 1):
    offsets.append(len(output))
    output.extend(f"{number} 0 obj\n".encode() + obj + b"\nendobj\n")
xref = len(output)
output.extend(b"xref\n0 6\n0000000000 65535 f \n")
for offset in offsets:
    output.extend(f"{offset:010d} 00000 n \n".encode())
output.extend(f"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
Path("citation.pdf").write_bytes(output)
print("Created citation.pdf: page 1, 612 x 792 PDF units, native text at (50, 700)")
