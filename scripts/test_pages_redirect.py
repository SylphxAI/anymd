import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

spec = importlib.util.spec_from_file_location('redirect', Path(__file__).with_name('build-pages-redirect.py'))
redirect = importlib.util.module_from_spec(spec)
spec.loader.exec_module(redirect)


class PagesRedirect(unittest.TestCase):
    def test_artifact_contains_redirects_and_no_storefront(self):
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / 'pages'
            redirect.build(out)
            for route in ('index.html', 'pricing.html', 'recover/index.html', 'pro/thanks/index.html', 'vs/docling.html', 'guide/cli/index.html', '404.html'):
                page = (out / route).read_text()
                self.assertIn("location.replace('https://anymd.sylphx.com'", page)
                self.assertIn('location.search + location.hash', page)
                self.assertIn('name="referrer" content="no-referrer"', page)
            names = {f.name for f in out.rglob('*') if f.is_file()}
            self.assertTrue(all(name.endswith('.html') or name in {'.nojekyll', 'llms.txt'} for name in names))
            self.assertEqual((out / 'llms.txt').read_text().strip(), 'Anymd moved: https://anymd.sylphx.com/llms.txt')


if __name__ == '__main__':
    unittest.main()
