"""Read a single handbook record or setting prefix without loading unrelated text."""
import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent

def setting_entries():
    index = json.loads((ROOT / 'configuration/settings-registry.json').read_text())
    for group in index['groups']:
        yield from json.loads((ROOT / 'configuration' / group['path']).read_text())['entries']

def record_text(record):
    path = ROOT / record['path']
    lines = path.read_text().splitlines()
    start = next(i for i, line in enumerate(lines) if line == record['heading'])
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith('## ')), len(lines))
    return '\n'.join(lines[start:end]).rstrip() + '\n'

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('record', nargs='?', help='OP-34, ALG-RESTORE or WP-11')
    group.add_argument('--setting', metavar='KEY_OR_PREFIX')
    group.add_argument('--list', choices=['operations', 'algorithms', 'packages', 'settings'])
    args = parser.parse_args()
    if args.setting or args.list == 'settings':
        # Read only the selected top-level group for setting retrieval.
        if args.setting:
            family = args.setting.split('.')[0]
            index = json.loads((ROOT / 'configuration/settings-registry.json').read_text())
            matches = [g for g in index['groups'] if g['name'] == family]
            candidates = [e for g in matches for e in json.loads((ROOT / 'configuration' / g['path']).read_text())['entries']]
            selected = [e for e in candidates if e['key'] == args.setting or e['key'].startswith(args.setting.rstrip('.') + '.')]
            if not selected: parser.error('Unknown setting key/prefix: ' + args.setting)
            print(json.dumps(selected, ensure_ascii=False, indent=2))
        else:
            for e in setting_entries(): print(e['key'])
        return
    index = json.loads((ROOT / 'reading-map.json').read_text())['records']
    if args.list:
        prefix = {'operations': 'OP-', 'algorithms': 'ALG-', 'packages': 'WP-'}[args.list]
        for name, record in sorted(index.items()):
            if name.startswith(prefix): print(f"{name}\t{record['path']}\t{record['title']}")
        return
    ident = args.record.upper()
    if not re.fullmatch(r'(?:OP-\d{2}|ALG-[A-Z]+|WP-\d{2})', ident) or ident not in index:
        parser.error('Unknown record ID: ' + args.record)
    record = index[ident]
    print(f"Source: docs/cli/{record['path']}\n")
    print(record_text(record), end='')
    if record['requires']:
        print('\nPrerequisite files (read separately):')
        for path in record['requires']: print('docs/cli/' + path)

if __name__ == '__main__':
    main()
