---
'@sylphx/anymd': patch
---

Structured JSON reads (`inspect` structure, legacy `read_pdf` profiles, and `search_pdf` detail) now keep word spaces on TeX and other PDFs that position words instead of emitting space glyphs: text such as "Thedominantsequence" reads "The dominant sequence", phrase search finds "multi-head attention" with its page and box, and page text puts each line on its own line.
