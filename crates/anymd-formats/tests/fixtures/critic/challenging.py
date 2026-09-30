#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Write the challenging tracked-change documents as LibreOffice flat XML (.fodt).

Usage: uv run challenging.py synthetic

Each document is written by hand around one hard case: delimiter-like text,
Unicode, links, notes, lists, overlapping comments, adjacent and multi-paragraph
changes, headings, formatting, and tables with text boxes. Files are named
hard-<case>.fodt; turn them into Word and oracle results as synthesize.py says.
"""

import re
import sys
from pathlib import Path

AUTHORS = ["Ana Lima", "Bo Chen", "Carla Duarte", "Dev Patel", "Émile Zola-Ng"]


class Doc:
    """Builds one document: tracked-change regions, comments, and the body."""

    def __init__(self) -> None:
        self.regions: list[str] = []
        self.comments = 0
        self.minute = 0

    def info(self, author: int) -> str:
        """Change info for author number `author`, at a date that moves on."""
        self.minute += 5
        hour, minute = divmod(self.minute, 60)
        return (
            f"<office:change-info><dc:creator>{AUTHORS[author % len(AUTHORS)]}</dc:creator>"
            f"<dc:date>2026-10-01T{hour:02}:{minute:02}:00</dc:date></office:change-info>"
        )

    def region(self, kind: str, author: int, inner: str = "") -> str:
        id_ = f"ct{len(self.regions) + 1}"
        self.regions.append(
            f'<text:changed-region xml:id="{id_}" text:id="{id_}">'
            f"<text:{kind}>{self.info(author)}{inner}</text:{kind}></text:changed-region>"
        )
        return id_

    def ins(self, content: str, author: int = 0) -> str:
        """Inserted content, which may run over paragraph ends (`</text:p><text:p>`)."""
        id_ = self.region("insertion", author)
        return f'<text:change-start text:change-id="{id_}"/>{content}<text:change-end text:change-id="{id_}"/>'

    def fmt(self, content: str, author: int = 0) -> str:
        """A formatting-only change over `content`."""
        id_ = self.region("format-change", author)
        return f'<text:change-start text:change-id="{id_}"/>{content}<text:change-end text:change-id="{id_}"/>'

    def dele(self, *paragraphs: str, author: int = 1) -> str:
        """Deleted text; several pieces are several paragraphs (the breaks go too).
        A leading space is written as <text:s/>, which ODF keeps."""
        inner = "".join(
            f"<text:p>{'<text:s/>' + p[1:] if p.startswith(' ') else p}</text:p>" for p in paragraphs
        )
        return f'<text:change text:change-id="{self.region("deletion", author, inner)}"/>'

    def cut_break(self, author: int = 1) -> str:
        """A deleted paragraph break, at the end of the paragraph it joins."""
        return self.dele("", "", author=author)

    def comment(self, content: str, *notes: str, author: int = 2) -> str:
        """A comment on `content`; several notes are several paragraphs of it."""
        start, end = self.comment_parts(*notes, author=author)
        return start + content + end

    def comment_parts(self, *notes: str, author: int = 2) -> tuple[str, str]:
        self.comments += 1
        name = f"c{self.comments}"
        body = "".join(f"<text:p>{n}</text:p>" for n in notes) or "<text:p/>"
        info = self.info(author).replace("<office:change-info>", "").replace("</office:change-info>", "")
        return (
            f'<office:annotation office:name="{name}">{info}{body}</office:annotation>',
            f'<office:annotation-end office:name="{name}"/>',
        )

    def point(self, *notes: str, author: int = 3) -> str:
        """A comment on a point."""
        body = "".join(f"<text:p>{n}</text:p>" for n in notes)
        info = self.info(author).replace("<office:change-info>", "").replace("</office:change-info>", "")
        return f"<office:annotation>{info}{body}</office:annotation>"


def p(content: str, style: str = "") -> str:
    attribute = f' text:style-name="{style}"' if style else ""
    return f"<text:p{attribute}>{content}</text:p>"


def h(level: int, content: str) -> str:
    return f'<text:h text:outline-level="{level}">{content}</text:h>'


def b(text: str) -> str:
    return f'<text:span text:style-name="B">{text}</text:span>'


def i(text: str) -> str:
    return f'<text:span text:style-name="I">{text}</text:span>'


def link(href: str, content: str) -> str:
    return f'<text:a xlink:type="simple" xlink:href="{href}">{content}</text:a>'


def note(kind: str, number: int, content: str) -> str:
    return (
        f'<text:note text:id="{kind}{number}" text:note-class="{kind}"><text:note-citation>{number}</text:note-citation>'
        f"<text:note-body>{content}</text:note-body></text:note>"
    )


def delimiters(d: Doc) -> list[str]:
    return [
        h(1, "Delimiters in the text"),
        p("Plain text with {++ this ++}, {-- that --}, {~~ a ~&gt; b ~~}, {== mark ==} and {&gt;&gt; note &lt;&lt;}."),
        p("Inserted: " + d.ins("x {++ y ++} z --} w ~&gt; v") + " and deleted: " + d.dele("a {-- b --} c ==}") + "."),
        p("A comment " + d.comment("on {== text ==}", "Note with &lt;&lt;} and {&gt;&gt; inside", "and ++} too") + " here."),
        p("Split across runs: a -" + b("-}") + " and {" + i("++") + " and ~" + b("&gt;") + " end."),
        p("An insertion ending in a dash" + d.ins(" -") + "-} right after it."),
        p("Math-like text " + d.ins("$x_{--}$ and $y^{++}$") + " stays."),
    ]


def unicode(d: Doc) -> list[str]:
    return [
        h(1, "Unicode everywhere"),
        p("中文" + d.ins("插入的文字") + "和" + d.dele("删除的文字") + "。日本語" + d.comment("のコメント", "コメント本文") + "です。"),
        p("עברית " + d.ins("טקסט חדש") + " ו" + d.dele("טקסט ישן") + ". العربية " + d.comment("نص", "تعليق") + "."),
        p("Emoji 👍🏽 " + d.ins("🚀 launch 🎉") + " and " + d.dele("👨‍👩‍👧 family") + " done."),
        p("Combining: é " + d.ins("ño") + " and non breaking" + d.dele(" space") + "."),
        p("Tabs<text:tab/>" + d.ins("inside<text:tab/>an insertion") + "<text:tab/>end."),
        p("A line" + d.ins("<text:line-break/>break inserted") + " and one" + d.dele("deleted") + "."),
    ]


def links(d: Doc) -> list[str]:
    return [
        h(1, "Links"),
        p("See " + link("https://example.com/a", "the " + d.ins("new ") + "page") + " for details."),
        p("An inserted link: " + d.ins(link("https://example.com/b", "fresh link")) + "."),
        p("A deleted link: " + d.dele("old link") + link("https://example.com/c", "kept link") + "."),
        p("A link whose text is replaced: " + link("https://example.com/d", d.dele("before") + d.ins("after", author=2)) + "."),
        p("A comment on " + d.comment(link("https://example.com/e", "a link"), "Is this the right URL?") + "."),
    ]


def notes(d: Doc) -> list[str]:
    return [
        h(1, "Notes"),
        p("A claim" + note("footnote", 1, p("Source " + d.ins("updated ") + "2024" + d.dele(", page 4") + ".")) + " stands."),
        p("An inserted note reference" + d.ins(note("footnote", 2, p("Added later."))) + " here."),
        p("A deleted sentence with a note" + d.dele(" goes away") + note("footnote", 3, p("Still here.")) + "."),
        p("An endnote" + note("endnote", 1, p("End " + d.comment("note", "Comment inside an endnote") + " text.") + p("Second paragraph" + d.cut_break() + " joined.")) + " too."),
        p("Comment " + d.comment("around a note" + note("footnote", 4, p("Inside a range.")), "Covers a note") + "."),
    ]


def lists(d: Doc) -> list[str]:
    def item(content: str, nested: str = "") -> str:
        return f"<text:list-item>{p(content)}{nested}</text:list-item>"

    nested = f'<text:list>{item("Nested " + d.ins("inserted"))}{item(d.dele("Nested deleted item") + "Nested kept")}</text:list>'
    return [
        h(1, "Lists"),
        p("Before the list."),
        '<text:list text:style-name="N1">'
        + item("First " + d.ins("new ") + "item")
        + item("Second item" + d.cut_break(), nested)
        + item("Third " + d.dele("old ") + "item")
        + item(d.ins("Inserted") + " fourth" + d.comment(" item", "Check numbering"))
        + "</text:list>",
        p("After the list."),
        '<text:list text:style-name="L1">'
        + item("Bullet " + d.dele("one"))
        + item("Bullet " + d.ins("two"))
        + "</text:list>",
    ]


def comments(d: Doc) -> list[str]:
    s1, e1 = d.comment_parts("First of two overlapping comments", author=0)
    s2, e2 = d.comment_parts("Second, overlapping the first", author=1)
    s3, e3 = d.comment_parts("Runs over three paragraphs", "It has two paragraphs itself", author=3)
    empty = d.comment("an empty comment", author=4)
    return [
        h(1, "Comments"),
        p("Overlap: " + s1 + "one two " + s2 + "three four" + e1 + " five six" + e2 + " seven."),
        p("Start " + s3 + "of a long range."),
        p("The middle paragraph."),
        p("The end" + e3 + " of it."),
        p(d.point("A point comment at the start") + "Point at start, " + empty + ", and at the end." + d.point("Last", "word")),
        p("A comment on " + d.comment(d.dele("deleted text"), "Why delete?") + " and on " + d.comment(d.ins("inserted text"), "Good.") + "."),
        p(d.comment("A whole paragraph under one comment.", "Whole paragraph")),
    ]


def adjacent(d: Doc) -> list[str]:
    return [
        h(1, "Adjacent and spanning changes"),
        p("Back to back: " + d.ins("one", 0) + d.ins("two", 1) + d.dele("three", author=2) + d.dele("four", author=3) + "."),
        p(d.ins("Start of paragraph ") + "middle" + d.dele(" end of paragraph")),
        p("Replaced by two people: " + d.dele("old", author=0) + d.ins("new", 1) + "."),
        p("Deletion across paragraphs: alpha" + d.dele(" beta", "gamma") + " delta."),
        p("Insertion across paragraphs: one" + d.ins(" two</text:p><text:p>three") + " four."),
        p("Before three cut paragraphs."),
        p(d.dele("Cut one.", "Cut two.", "Cut three.", "") + "After them."),
        p(""),
        p("An empty paragraph above" + d.cut_break() + " and a joined one."),
        p("Last" + d.ins(" words")),
    ]


def headings(d: Doc) -> list[str]:
    return [
        h(1, "Heading " + d.ins("with an insertion")),
        p("Body text."),
        h(2, d.dele("Deleted ") + "Heading two"),
        p("More text" + d.cut_break()),
        h(2, "Heading joined to the text above"),
        p("Text before an inserted heading." + d.ins("</text:p>" + h(2, "Inserted heading").replace("</text:h>", "") + "</text:h><text:p>")),
        p("Text after it."),
        h(3, d.comment("Commented heading", "Rename?")),
    ]


def formatting(d: Doc) -> list[str]:
    return [
        h(1, "Formatting"),
        p("Bold inserted: " + d.ins(b("strong") + " and " + i("slanted")) + " text."),
        p("Deleted bold: " + d.dele("heavy") + " text."),
        p("Mid-word: con" + d.ins(b("tract")) + "ual and wo" + d.dele("rd") + "s."),
        p("Formatting change only: " + d.fmt(b("now bold")) + " and " + d.fmt(i("now italic"), 2) + "."),
        p("Bold " + b("around " + d.ins("an insertion") + " inside") + " end."),
        p(i("Italic paragraph with a " + d.dele("deleted") + " word.")),
    ]


def tables(d: Doc) -> list[str]:
    cell = lambda content, extra="": f'<table:table-cell office:value-type="string"{extra}>{content}</table:table-cell>'
    covered = "<table:covered-table-cell/>"
    span = ' table:number-columns-spanned="2"'
    nested = (
        '<table:table table:name="N"><table:table-column table:number-columns-repeated="2"/>'
        f"<table:table-row>{cell(p('in ' + d.ins('nested')))}{cell(p(d.dele('gone')))}</table:table-row>"
        "</table:table>"
    )
    frame = (
        '<draw:frame draw:name="Box" text:anchor-type="as-char" svg:width="4cm" svg:height="2cm">'
        f"<draw:text-box>{p('Box ' + d.ins('inserted') + ' and ' + d.dele('deleted') + '.')}</draw:text-box></draw:frame>"
    )
    return [
        h(1, "Tables and text boxes"),
        '<table:table table:name="T"><table:table-column table:number-columns-repeated="3"/>'
        f"<table:table-row>{cell(p('Merged ' + d.ins('header')), span)}{covered}{cell(p('C'))}</table:table-row>"
        f"<table:table-row>{cell(p('Line one' + d.cut_break()) + p('line two'))}{cell(p('Para') + p(d.ins('Added para')))}{cell(p(d.comment('noted', 'Cell comment')))}</table:table-row>"
        f"<table:table-row>{cell(nested)}{cell(p(''))}{cell(p(d.dele('all deleted')))}</table:table-row>"
        "</table:table>",
        p("A paragraph with a text box " + frame + " inside."),
        p("A deleted text box" + d.dele("gone") + " and after."),
    ]


CASES = {
    "delimiters": delimiters,
    "unicode": unicode,
    "links": links,
    "notes": notes,
    "lists": lists,
    "comments": comments,
    "adjacent": adjacent,
    "headings": headings,
    "formatting": formatting,
    "tables": tables,
}


def document(build) -> str:
    d = Doc()
    # ODF folds a space right after a deletion into the one before it; the
    # space after the deleted text is meant, so it is written as <text:s/>.
    content = re.sub(r'(<text:change text:change-id="ct\d+"/>) ', r"\1<text:s/>", "\n   ".join(build(d)))
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.text">
 <office:automatic-styles>
  <style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
  <style:style style:name="I" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
  <text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/><text:list-level-style-bullet text:level="2" text:bullet-char="◦"/></text:list-style>
  <text:list-style style:name="N1"><text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."/><text:list-level-style-number text:level="2" style:num-format="a" style:num-suffix="."/></text:list-style>
 </office:automatic-styles>
 <office:body>
  <office:text>
   <text:tracked-changes text:track-changes="true">{"".join(d.regions)}</text:tracked-changes>
   {content}
  </office:text>
 </office:body>
</office:document>
"""


def main(out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    for name, build in CASES.items():
        (out / f"hard-{name}.fodt").write_text(document(build), encoding="utf-8")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
