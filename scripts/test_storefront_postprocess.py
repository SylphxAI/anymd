#!/usr/bin/env python3
"""Tests for storefront_postprocess.py SEO, layout, and click-ID patching."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import storefront_postprocess as sp  # noqa: E402


class StorefrontPostprocessTest(unittest.TestCase):
    def test_deduplicate_title(self) -> None:
        self.assertEqual(
            sp.deduplicate_title(
                "anymd Pro: verifiable video evidence and cite-check for AI agents · anymd Pro"
            ),
            "anymd Pro: verifiable video evidence and cite-check for AI agents",
        )
        self.assertEqual(
            sp.deduplicate_title("Recover your anymd Pro licence · anymd Pro"),
            "Recover your anymd Pro licence · anymd Pro",
        )

    def test_postprocess_pricing_html_is_idempotent(self) -> None:
        root = Path(__file__).resolve().parent.parent
        html = (root / "docs" / "storefront" / "prebuilt" / "pricing.html").read_text(
            encoding="utf-8"
        )
        fm = sp.parse_frontmatter(root / "docs" / "storefront" / "pricing.md")
        once = sp.postprocess_pricing_html(html, fm)
        twice = sp.postprocess_pricing_html(once, fm)
        self.assertEqual(once, twice)
        self.assertIn(
            '<meta property="og:image" content="https://sylphxai.github.io/anymd/og-pro.png">',
            once,
        )
        self.assertNotIn("· anymd Pro</title>", once)
        self.assertNotIn('"price":"0"', once)

    def test_postprocess_money_js_is_idempotent(self) -> None:
        root = Path(__file__).resolve().parent.parent
        money_files = list((root / "docs" / "storefront" / "prebuilt" / "_keel").glob("money.*.js"))
        self.assertTrue(money_files)
        js = money_files[0].read_text(encoding="utf-8")
        once = sp.postprocess_money_js(js)
        twice = sp.postprocess_money_js(once)
        self.assertEqual(once, twice)
        for key in ("gclid", "gbraid", "wbraid", "fbclid"):
            self.assertIn(f'"{key}"', once)


if __name__ == "__main__":
    unittest.main()
