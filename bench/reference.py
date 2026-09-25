#!/usr/bin/env python3
"""Build the reference texts in bench/reference/ that text F1 is scored against.

A reference text is the document's words, independent of any Markdown layout:

  pdf (born-digital)  the PDF text layer via poppler `pdftotext -enc UTF-8`
  docx                every w:t run of word/document.xml, one paragraph per line
  pptx                every a:t run of each slide, then its speaker notes, in deck order
  epub                the text of every spine document, in spine order

Scanned PDFs, spreadsheets, CSV, and HTML have no reference text: scanned
pages have no text layer, and spreadsheets and HTML are scored on their
tables, sentences, and headings instead. The files are committed, so scores do
not depend on the poppler version installed where the benchmark runs.

  python bench/reference.py CORPUS_DIR [doc-id ...]
"""

import html
import json
import re
import subprocess
import sys
import zipfile
from html.parser import HTMLParser
from pathlib import Path, PurePosixPath

HERE = Path(__file__).resolve().parent


class _Text(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts, self._skip = [], 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style", "head"):
            self._skip += 1
        elif tag in ("p", "div", "br", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6"):
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in ("script", "style", "head"):
            self._skip -= 1

    def handle_data(self, data):
        if not self._skip:
            self.parts.append(data)


def xml_runs(xml, run_tag, para_tag):
    text = re.sub(rf"</{para_tag}>", "\n", xml)
    text = re.sub(r"<w:tab/>|<w:br/>", " ", text)
    runs = re.findall(rf"<{run_tag}(?: [^>]*)?>([^<]*)</{run_tag}>|(\n)", text)
    return "".join(a or b for a, b in runs)


def unescape(text):
    return html.unescape(text)


def docx(path):
    with zipfile.ZipFile(path) as z:
        return unescape(xml_runs(z.read("word/document.xml").decode("utf-8"), "w:t", "w:p"))


def pptx(path):
    with zipfile.ZipFile(path) as z:
        pres = z.read("ppt/presentation.xml").decode("utf-8")
        rels = z.read("ppt/_rels/presentation.xml.rels").decode("utf-8")
        targets = {}
        for rel in re.findall(r"<Relationship\b[^>]*>", rels):
            targets[re.search(r'Id="([^"]+)"', rel).group(1)] = re.search(r'Target="([^"]+)"', rel).group(1)
        out = []
        for rid in re.findall(r"<p:sldId [^>]*r:id=\"([^\"]+)\"", pres):
            slide = "ppt/" + targets[rid].lstrip("/").removeprefix("ppt/")
            out.append(xml_runs(z.read(slide).decode("utf-8"), "a:t", "a:p"))
            slide_rels = str(PurePosixPath(slide).parent / "_rels" / (PurePosixPath(slide).name + ".rels"))
            if slide_rels in z.namelist():
                for target in re.findall(r'Target="([^"]*notesSlide[^"]*)"', z.read(slide_rels).decode("utf-8")):
                    notes = re.sub(r"[^/]+/\.\./", "", str(PurePosixPath(slide).parent / target))
                    if notes in z.namelist():
                        body = z.read(notes).decode("utf-8")
                        body = re.sub(r"<a:fld [^>]*type=\"slidenum\".*?</a:fld>", "", body, flags=re.S)
                        out.append(xml_runs(body, "a:t", "a:p"))
        return unescape("\n".join(out))


def epub(path):
    with zipfile.ZipFile(path) as z:
        container = z.read("META-INF/container.xml").decode("utf-8")
        opf_path = re.search(r'full-path="([^"]+)"', container).group(1)
        opf = z.read(opf_path).decode("utf-8")
        base = PurePosixPath(opf_path).parent
        items = {}
        for tag in re.findall(r"<(?:\w+:)?item\b[^>]*>", opf):
            item_id = re.search(r'\bid="([^"]+)"', tag)
            href = re.search(r'\bhref="([^"]+)"', tag)
            if item_id and href:
                items[item_id.group(1)] = href.group(1)
        out = []
        for idref in re.findall(r'<(?:\w+:)?itemref\b[^>]*idref="([^"]+)"', opf):
            name = str(base / items[idref]) if str(base) != "." else items[idref]
            parser = _Text()
            parser.feed(z.read(name).decode("utf-8", "replace"))
            out.append("".join(parser.parts))
        return "\n".join(out)


def pdf(path):
    return subprocess.run(["pdftotext", "-enc", "UTF-8", str(path), "-"], capture_output=True, check=True).stdout.decode("utf-8")


def main():
    corpus, *only = sys.argv[1:]
    out_dir = HERE / "reference"
    out_dir.mkdir(exist_ok=True)
    for doc in json.loads((HERE / "corpus.json").read_text("utf-8"))["docs"]:
        if only and doc["id"] not in only:
            continue
        builder = {"docx": docx, "pptx": pptx, "epub": epub, "pdf": pdf}.get(doc["format"])
        if builder is None or doc.get("ocr"):
            continue
        text = builder(Path(corpus) / doc["file"])
        text = re.sub(r"[ \t]+\n", "\n", text.replace("\f", "\n"))
        text = re.sub(r"\n{3,}", "\n\n", text).strip() + "\n"
        (out_dir / f"{doc['id']}.txt").write_text(text, "utf-8")
        print(f"{doc['id']}: {len(text.split())} words")


if __name__ == "__main__":
    main()
