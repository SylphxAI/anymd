"""Local scorer regressions: python -m unittest discover -s bench-asr -p test_report.py."""

import unittest
import report


class MandarinDiagnosticsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # No downloaded English normalizer is needed for these Chinese tests.
        import unicodedata
        from opencc import OpenCC

        t2s = OpenCC("t2s")

        def cjk(text):
            text = unicodedata.normalize("NFKC", text).lower()
            return t2s.convert("".join(c for c in text if unicodedata.category(c)[0] not in "PSZC"))

        cls.cjk = staticmethod(cjk)
        annotations, numerals = report.mandarin_diagnostics(cjk)
        cls.annotations = staticmethod(annotations)
        cls.numerals = staticmethod(numerals)

    def test_annotations_removed_before_punctuation(self):
        self.assertEqual(self.annotations("邓迪大学（University of Dundee）的教授"), "邓迪大学的教授")

    def test_accented_latin_annotation(self):
        self.assertEqual(self.annotations("山谷 (Cochamó Valley)"), "山谷")

    def test_chinese_and_numeric_parentheses_preserved(self):
        self.assertEqual(self.annotations("（教养良好）（31英里）（1000-1300年）"), "教养良好31英里10001300年")
        self.assertEqual(self.annotations("（M16步枪）"), "m16步枪")

    def test_unparenthesized_latin_preserved(self):
        self.assertEqual(self.annotations("Pamela Ferguson 教授"), "pamelaferguson教授")

    def test_numeral_ablation_is_symmetric(self):
        ref = "桥下垂直净空15米，2011年8月完工。"
        hyp = "桥下垂直净空十五米，二零一一年八月完工。"
        self.assertEqual(self.numerals(ref), self.numerals(hyp))
        self.assertNotEqual(self.cjk(ref), self.cjk(hyp))

    def test_reference_annotation_changes_denominator(self):
        ref, hyp = "大学（University）", "大学"
        self.assertEqual(report.edit_stats(self.cjk(ref), self.cjk(hyp), "char"), (10, 12))
        self.assertEqual(report.edit_stats(self.annotations(ref), self.annotations(hyp), "char"), (0, 2))


if __name__ == "__main__":
    unittest.main()
