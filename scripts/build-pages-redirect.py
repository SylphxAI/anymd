#!/usr/bin/env python3
"""Build redirect-only GitHub Pages compatibility files; no storefront assets."""
from html import escape
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
ORIGIN = 'https://anymd.sylphx.com'


def paths():
    yield from ('', 'pricing', 'docs', 'playground', 'pro', 'pro/thanks', 'recover', 'vs')
    for folder in ('guide', 'legal', 'security', 'tasks', 'use-cases', 'vs'):
        yield from (f'{folder}/{p.stem}' for p in (ROOT / 'docs' / folder).glob('*.md') if p.stem != 'index')
    yield 'vision'


def page(destination, fallback=False):
    target = escape(ORIGIN + destination, quote=True)
    path = "location.pathname.replace(/^\\/anymd(?=\\/|$)/, '').replace(/\\/index\\.html$/, '/').replace(/\\.html$/, '')" if fallback else repr(destination)
    return f'''<!doctype html>
<html lang="en"><meta charset="utf-8"><meta name="robots" content="noindex">
<meta name="referrer" content="no-referrer"><title>Anymd has moved</title>
<link rel="canonical" href="{target}">
<script>location.replace('{ORIGIN}' + ({path} || '/') + location.search + location.hash);</script>
<body><p>Anymd has moved to <a href="{target}">{target}</a>.</p></body></html>
'''


def build(out):
    # Refuse to overwrite an existing directory that could contain a site.
    out.mkdir(parents=True, exist_ok=False)
    (out / '.nojekyll').touch()
    for route in paths():
        dst = '/' + route
        index = out / route / 'index.html'
        index.parent.mkdir(parents=True, exist_ok=True)
        index.write_text(page(dst), encoding='utf-8')
        if route:
            (out / f'{route}.html').write_text(page(dst), encoding='utf-8')
    (out / '404.html').write_text(page('/', fallback=True), encoding='utf-8')
    # One URL-only pointer for non-browser clients; the actual index is on Next.
    (out / 'llms.txt').write_text(f'Anymd moved: {ORIGIN}/llms.txt\n', encoding='utf-8')


if __name__ == '__main__':
    build(Path(sys.argv[1]))
