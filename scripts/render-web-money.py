#!/usr/bin/env python3
import argparse
import json
from pathlib import Path
import tomllib

root = Path(__file__).resolve().parents[1]
source = root / 'crates/anymd/src/money_docs.toml'
money = tomllib.loads(source.read_text())['money']
outputs = {
    'money-production.generated.ts': {
        'productionMoney': {'origin': money['api_origin'], 'parent': money['parent'], 'pk': money['publishable_key']},
    },
    'money-public.generated.ts': {
        'consentText': money['offers']['pro']['consent_text'],
        'consentVersion': money['offers']['pro']['consent_version'],
    },
}
parser = argparse.ArgumentParser()
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
for name, constants in outputs.items():
    file = root / 'apps/web/src/lib' / name
    text = '// Generated from crates/anymd/src/money_docs.toml by scripts/render-web-money.py.\n'
    if name == 'money-production.generated.ts':
        text += "import 'server-only';\n"
    for key, value in constants.items():
        text += f'export const {key} = {json.dumps(value)};\n'
    if args.check:
        if not file.is_file() or file.read_text() != text:
            raise SystemExit(f'{file.name} is stale; run python3 scripts/render-web-money.py')
    else:
        file.write_text(text)
