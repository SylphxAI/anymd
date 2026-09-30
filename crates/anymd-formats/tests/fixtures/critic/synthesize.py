#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Write the synthetic tracked-change documents as LibreOffice flat XML (.fodt).

Usage: uv run synthesize.py synthetic

Each profile is one document, heavy on one kind of change or a mix of them.
The text and the changes come from a seeded random generator, so the output is
the same on every run. Save each .fodt as Word with LibreOffice, then write
Writer's Accept All and Reject All results with libreoffice-oracle.py:

  cd synthetic
  soffice --headless --convert-to "docx:MS Word 2007 XML" *.fodt
  for f in *.fodt; do
    /usr/bin/python3 ../libreoffice-oracle.py "$f" /tmp/lo-profile --docx
  done

Accept All and Reject All run on the .fodt, not on the .docx (see
libreoffice-oracle.py).
"""

import random
import sys
from dataclasses import dataclass, field
from pathlib import Path

AUTHORS = ["Ana Lima", "Bo Chen", "Carla Duarte", "Dev Patel"]

SENTENCES = [
    "The supplier shall deliver the goods within thirty days of the order",
    "Payment is due on receipt of a correct invoice",
    "Either party may end this agreement with ninety days written notice",
    "The buyer may inspect the goods before accepting them",
    "Late payments carry interest at the statutory rate",
    "Each party keeps the other party's confidential information secret",
    "This agreement is governed by the laws of the State of New York",
    "Notices must be sent by email and by registered mail",
    "The warranty period is twelve months from delivery",
    "Neither party is liable for delays caused by events beyond its control",
    "The supplier keeps title to the goods until they are paid in full",
    "Disputes go first to mediation and then to the courts of Manhattan",
    "Any change to this agreement must be in writing and signed by both parties",
    "The buyer shall return defective goods within fourteen days",
    "The price includes packaging but excludes shipping and taxes",
    "The supplier may subcontract only with the buyer's prior consent",
]

INSERTS = [
    "promptly",
    "in good faith",
    "at its own cost",
    "without undue delay",
    "unless agreed otherwise",
    "as reasonably required",
    "in full",
    "on a business day",
]

NOTES = [
    "Is this consistent with the master agreement?",
    "Please confirm with finance.",
    "Too broad, narrow it down.",
    "Agreed.",
    "Legal to review.",
    "Why thirty and not sixty?",
    "Check the defined term.",
    "Client asked for this.",
]


@dataclass
class Profile:
    """How often each kind of change is drawn, per paragraph or per word."""

    name: str
    title: str
    insert: float = 0.0  # per word: text inserted after it
    delete: float = 0.0  # per word: the word deleted
    substitute: float = 0.0  # per word: the word replaced
    comment: float = 0.0  # per paragraph: a comment on a few words
    point_comment: float = 0.0  # per paragraph: a comment on a point
    cross_comment: float = 0.0  # per paragraph: a comment running into the next
    new_paragraph: float = 0.0  # per paragraph: a whole inserted paragraph after it
    cut_paragraph: float = 0.0  # per paragraph: the paragraph deleted whole
    split: float = 0.0  # per paragraph: an inserted break inside it
    join: float = 0.0  # per paragraph: its break deleted, joining the next one
    paragraphs: int = 14
    table: bool = False
    lists: bool = False
    footnotes: bool = False


PROFILES = [
    Profile(
        "all",
        "Heavy on everything",
        insert=0.08,
        delete=0.08,
        substitute=0.05,
        comment=0.5,
        point_comment=0.2,
        cross_comment=0.1,
        new_paragraph=0.15,
        cut_paragraph=0.1,
        split=0.1,
        join=0.1,
        table=True,
        lists=True,
        footnotes=True,
    ),
    Profile(
        "comments",
        "Heavy on comments",
        comment=0.9,
        point_comment=0.5,
        cross_comment=0.25,
    ),
    Profile(
        "deletions",
        "Heavy on deletions",
        delete=0.2,
        cut_paragraph=0.2,
        join=0.15,
    ),
    Profile(
        "additions",
        "Heavy on additions",
        insert=0.2,
        new_paragraph=0.3,
        split=0.15,
    ),
    Profile(
        "comments-deletions",
        "Comments and deletions",
        delete=0.15,
        cut_paragraph=0.1,
        comment=0.7,
        point_comment=0.3,
    ),
    Profile(
        "comments-additions",
        "Comments and additions",
        insert=0.15,
        new_paragraph=0.2,
        comment=0.7,
        point_comment=0.3,
    ),
    Profile(
        "substitutions",
        "Deletions next to additions",
        substitute=0.2,
        insert=0.05,
        delete=0.05,
    ),
    Profile(
        "breaks",
        "Paragraph breaks inserted and deleted",
        split=0.4,
        join=0.3,
        new_paragraph=0.15,
        cut_paragraph=0.15,
    ),
    Profile(
        "tables",
        "Changes and comments in tables",
        insert=0.1,
        delete=0.1,
        substitute=0.05,
        comment=0.3,
        paragraphs=4,
        table=True,
    ),
    Profile(
        "structure",
        "Changes in headings, lists and footnotes",
        insert=0.1,
        delete=0.1,
        substitute=0.05,
        comment=0.3,
        split=0.1,
        join=0.1,
        paragraphs=8,
        lists=True,
        footnotes=True,
    ),
]


@dataclass
class Writer:
    """Collects the tracked-change regions and the body of one document."""

    random: random.Random
    regions: list[str] = field(default_factory=list)
    comments: int = 0
    minute: int = 0

    def info(self) -> str:
        """A change or comment's author and a date that moves on each time."""
        self.minute += 7
        day, rest = divmod(self.minute, 24 * 60)
        hour, minute = divmod(rest, 60)
        author = self.random.choice(AUTHORS)
        date = f"2026-09-{1 + day % 28:02}T{hour:02}:{minute:02}:00"
        return (
            f"<office:change-info><dc:creator>{author}</dc:creator>"
            f"<dc:date>{date}</dc:date></office:change-info>"
        )

    def region(self, kind: str, inner: str = "") -> str:
        """Records a changed region and returns its id."""
        id_ = f"ct{len(self.regions) + 1}"
        self.regions.append(
            f'<text:changed-region xml:id="{id_}" text:id="{id_}">'
            f"<text:{kind}>{self.info()}{inner}</text:{kind}></text:changed-region>"
        )
        return id_

    def inserted(self, content: str) -> str:
        """`content` as tracked inserted text."""
        id_ = self.region("insertion")
        return (
            f'<text:change-start text:change-id="{id_}"/>{content}'
            f'<text:change-end text:change-id="{id_}"/>'
        )

    def deleted(self, text: str) -> str:
        """`text` as tracked deleted text. A leading space is written as
        `<text:s/>`, which ODF keeps at the start of a paragraph."""
        if text.startswith(" "):
            text = "<text:s/>" + text[1:]
        return f'<text:change text:change-id="{self.region("deletion", f"<text:p>{text}</text:p>")}"/>'

    def break_start(self) -> tuple[str, str]:
        """An inserted paragraph break: the end of one paragraph, the start of the next."""
        id_ = self.region("insertion")
        return (
            f'<text:change-start text:change-id="{id_}"/>',
            f'<text:change-end text:change-id="{id_}"/>',
        )

    def cut(self, paragraphs: list[str]) -> str:
        """Paragraphs deleted whole, marked at the start of the next one."""
        inner = "".join(f"<text:p>{p}</text:p>" for p in paragraphs) + "<text:p/>"
        return f'<text:change text:change-id="{self.region("deletion", inner)}"/>'

    def annotation(self, note: str) -> tuple[str, str]:
        """A comment's start (with its text) and the end of its range."""
        self.comments += 1
        name = f"c{self.comments}"
        info = self.info().replace("office:change-info", "x").replace("<x>", "").replace("</x>", "")
        start = (
            f'<office:annotation office:name="{name}">{info}'
            f"<text:p>{note}</text:p></office:annotation>"
        )
        return start, f'<office:annotation-end office:name="{name}"/>'

    def point_comment(self, note: str) -> str:
        """A comment on a point, with no range."""
        info = self.info().replace("office:change-info", "x").replace("<x>", "").replace("</x>", "")
        return f"<office:annotation>{info}<text:p>{note}</text:p></office:annotation>"


def words(writer: Writer, profile: Profile, sentence: str, style: bool = True) -> list[str]:
    """One sentence as a list of words with their insertions, deletions and
    substitutions; the last word carries the full stop."""
    rng = writer.random
    out = []
    visible = False  # ODF drops a plain space with no visible text before it
    for index, word in enumerate(sentence.split(" ")):
        roll = rng.random()
        space = (" " if visible else "<text:s/>") if index else ""
        if style and rng.random() < 0.08:
            word = f'<text:span text:style-name="{rng.choice("BI")}">{word}</text:span>'
        if roll < profile.delete:
            token = writer.deleted(" " * bool(index) + word)
        elif roll < profile.delete + profile.substitute:
            new = rng.choice(["supplier", "vendor", "customer", "notice", "sixty", "written"])
            token = space + writer.deleted(word) + writer.inserted(new)
            visible = True
        else:
            token = space + word
            visible = True
            if rng.random() < profile.insert:
                token += writer.inserted(" " + rng.choice(INSERTS))
        out.append(token)
    out[-1] += "."
    return out


def commented(writer: Writer, profile: Profile, tokens: list[str]) -> str:
    """Joins the words, adding a comment on a run of them and one on a point,
    as the profile draws."""
    rng = writer.random
    tokens = list(tokens)
    if rng.random() < profile.comment and len(tokens) > 1:
        first = rng.randrange(len(tokens) - 1)
        last = rng.randrange(first, min(first + 4, len(tokens)))
        start, end = writer.annotation(rng.choice(NOTES))
        tokens[first] = start + tokens[first]
        tokens[last] = tokens[last] + end
    text = "".join(tokens)
    if rng.random() < profile.point_comment:
        text += writer.point_comment(rng.choice(NOTES))
    return text


def body(profile: Profile, seed: int) -> tuple[str, str]:
    """The tracked-change regions and the body of one document."""
    writer = Writer(random.Random(seed))
    rng = writer.random
    out = [f'<text:h text:outline-level="1">{profile.title}</text:h>']
    pending_start = ""  # the start of the next paragraph, for an inserted or cut break
    open_comment = ""  # the end of a comment range running into the next paragraph
    sentences = rng.sample(SENTENCES * 2, profile.paragraphs)
    joining = False  # the previous paragraph's break was deleted
    for index, sentence in enumerate(sentences):
        tokens = words(writer, profile, sentence)
        last = index == len(sentences) - 1
        if not last and rng.random() < profile.split:
            # An inserted break between two words; the space stays before it.
            at = rng.randrange(1, len(tokens))
            start, end = writer.break_start()
            space = " " if tokens[at].startswith(" ") else ""
            tokens[at] = f"{space}{start}</text:p><text:p>{end}{tokens[at][len(space):]}"
        text = commented(writer, profile, tokens)
        text = pending_start + open_comment + text
        pending_start = open_comment = ""
        if not last and rng.random() < profile.cross_comment:
            start, open_comment = writer.annotation(rng.choice(NOTES))
            text += start
        cut = not joining and not last and rng.random() < profile.cut_paragraph
        # A paragraph holding a comment (or a comment's end) is not cut: the
        # deleted copy is plain text, and the comment would lose an end.
        if cut and "<office:annotation" not in text:
            # The paragraph is deleted whole: Word keeps it with a deleted break.
            pending_start = writer.cut([sentence + "."]) + open_comment
            open_comment = ""
            continue
        if joining:
            out[-1] = out[-1][: -len("</text:p>")] + " " + text + "</text:p>"
        else:
            out.append(f"<text:p>{text}</text:p>")
        joining = False
        if not last and rng.random() < profile.join:
            # A deleted break: this paragraph and the next one become one.
            id_ = writer.region("deletion", "<text:p/><text:p/>")
            out[-1] = out[-1][: -len("</text:p>")] + f'<text:change text:change-id="{id_}"/></text:p>'
            joining = True
            continue
        if not last and rng.random() < profile.new_paragraph:
            start, end = writer.break_start()
            added = rng.choice(SENTENCES)
            out[-1] = out[-1][: -len("</text:p>")] + f"{start}</text:p>"
            out.append(f"<text:p>{end}{writer.inserted(added + '.')}</text:p>")
        if index == 2 and profile.lists:
            items = "".join(
                f"<text:list-item><text:p>{commented(writer, profile, words(writer, profile, s, False))}</text:p></text:list-item>"
                for s in rng.sample(SENTENCES, 4)
            )
            out.append(f'<text:list text:style-name="L1">{items}</text:list>')
        if index == 3 and profile.footnotes:
            note = "".join(words(writer, profile, rng.choice(SENTENCES), False))
            out[-1] = out[-1][: -len("</text:p>")] + (
                f'<text:note text:id="n{index}" text:note-class="footnote"><text:note-citation>1</text:note-citation>'
                f"<text:note-body><text:p>{note}</text:p></text:note-body></text:note></text:p>"
            )
        if index == 1 and profile.table:
            out.append(table(writer, profile))
    if pending_start or open_comment:
        out.append(f"<text:p>{pending_start}{open_comment}The end.</text:p>")
    return "".join(writer.regions), "\n   ".join(out)


def table(writer: Writer, profile: Profile) -> str:
    """A table whose cells hold tracked changes and comments."""
    rng = writer.random
    rows = []
    for row in range(5):
        cells = []
        for column in range(3):
            if row == 0:
                text = ["Clause", "Change", "Status"][column]
            else:
                clause = " ".join(rng.choice(SENTENCES).split()[:4])
                text = commented(writer, profile, words(writer, profile, clause, False))
            cells.append(f'<table:table-cell office:value-type="string"><text:p>{text}</text:p></table:table-cell>')
        rows.append(f"<table:table-row>{''.join(cells)}</table:table-row>")
    return (
        '<table:table table:name="T1"><table:table-column table:number-columns-repeated="3"/>'
        + "".join(rows)
        + "</table:table>"
    )


def document(profile: Profile, seed: int) -> str:
    """The whole .fodt of one profile."""
    regions, content = body(profile, seed)
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.text">
 <office:automatic-styles>
  <style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
  <style:style style:name="I" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
  <text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/></text:list-style>
 </office:automatic-styles>
 <office:body>
  <office:text>
   <text:tracked-changes text:track-changes="true">{regions}</text:tracked-changes>
   {content}
  </office:text>
 </office:body>
</office:document>
"""


def main(out: Path) -> None:
    """Writes <profile>.fodt for every profile into `out`."""
    out.mkdir(parents=True, exist_ok=True)
    for seed, profile in enumerate(PROFILES, start=1):
        (out / f"{profile.name}.fodt").write_text(document(profile, seed), encoding="utf-8")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
