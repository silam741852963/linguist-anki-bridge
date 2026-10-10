"""Check handbook routes, grouped settings, links and archive layout; no Anki calls."""
from pathlib import Path
import argparse
import json
import re
import tomllib

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[1]
errors = []

def check(condition, message):
    if not condition:
        errors.append(message)

def flatten(value, prefix=''):
    for key, item in value.items():
        name = prefix + key
        if isinstance(item, dict):
            yield from flatten(item, name + '.')
        else:
            yield name, item

def slug(text):
    text = re.sub(r'[`*]', '', text).lower()
    text = re.sub(r'[^\w\s-]', '', text)
    return re.sub(r'\s+', '-', text.strip())

def build_routes(files):
    records = {}
    bodies = {}
    for p in files:
        for sec in re.split(r'(?=^## )', p.read_text(), flags=re.M):
            m = re.match(r'^## ((?:OP|WP)-\d{2}|ALG-[A-Z]+)\b', sec)
            if not m:
                continue
            ident = m.group(1)
            check(ident not in records, 'Duplicate ID: ' + ident)
            requires = ['contracts/invariants.md']
            if ident.startswith('OP-'): requires += ['operations/README.md']
            if ident.startswith('WP-'): requires += ['implementation/README.md']
            if p.parent.name == 'recovery' or ident in {'OP-17','OP-34','OP-39','OP-41','OP-42','OP-44','OP-50','OP-52','OP-60','WP-03','WP-10','WP-11','WP-12','WP-13'}:
                requires += ['recovery/README.md','recovery/journal.md','contracts/identity.md']
            records[ident] = {'path':str(p.relative_to(ROOT)), 'heading':sec.splitlines()[0], 'title':sec.splitlines()[0][3:], 'words':len(sec.split()), 'requires':list(dict.fromkeys(requires))}
            bodies[ident] = sec
    topics = {
        'WP-01':['storage-and-wire','release-gates'], 'WP-02':['ux-and-operations','learning-and-providers'],
        'WP-03':['native-bridge'], 'WP-04':['storage-and-wire'], 'WP-05':['learning-and-providers','ux-and-operations'],
        'WP-06':['learning-and-providers','release-gates'], 'WP-07':['learning-and-providers'], 'WP-08':['learning-and-providers'],
        'WP-09':['storage-and-wire','learning-and-providers','ux-and-operations'], 'WP-10':['native-bridge','ux-and-operations'],
        'WP-11':['native-bridge','ux-and-operations'], 'WP-12':['native-bridge','storage-and-wire'],
        'WP-13':['native-bridge','storage-and-wire','ux-and-operations'], 'WP-14':['storage-and-wire','release-gates'],
        'WP-15':['release-gates','learning-and-providers'], 'WP-16':['release-gates'], 'WP-17':['native-bridge','release-gates']}
    for ident, rec in records.items():
        selected = topics.get(ident, [])
        if ident.startswith('ALG-'):
            selected = ['native-bridge'] if rec['path'].startswith('recovery/') else ['learning-and-providers']
            if ident in {'ALG-GC','ALG-RENDER'}: selected = ['storage-and-wire']
            if ident == 'ALG-CONFIG': selected = ['ux-and-operations']
        if ident.startswith('OP-'):
            n = int(ident[3:])
            if n <= 10 or 25 <= n <= 33: selected = ['ux-and-operations']
            if 21 <= n <= 24: selected = ['learning-and-providers']
            if n in {17,34,39,41,42,44,50,52,60}: selected = ['native-bridge','ux-and-operations']
            if n == 58: selected = ['release-gates']
        rec['requires'] += ['decisions/'+topic+'.md' for topic in selected]
        for referenced in sorted(set(re.findall(r'\b(?:ALG-[A-Z]+|OP-\d{2})\b', bodies[ident]))):
            if referenced in records and referenced != ident:
                path = records[referenced]['path']
                if path != rec['path'] and path not in rec['requires']: rec['requires'].append(path)
    return {'schema_version':1, 'records':records}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--update-map', action='store_true', help='Regenerate ID metadata after edits')
    args = parser.parse_args()
    index = json.loads((ROOT / 'configuration/settings-registry.json').read_text())
    entries = []
    for group in index['groups']:
        path = ROOT / 'configuration' / group['path']
        check(path.resolve().is_relative_to(ROOT), 'Setting group escapes handbook')
        data = json.loads(path.read_text())
        check(len(data['entries']) == group['count'], 'Setting group count: ' + group['name'])
        check(all(e['key'].split('.')[0] == group['name'] for e in data['entries']), 'Setting group ownership: ' + group['name'])
        entries.extend(data['entries'])
    by_key = {e['key']:e for e in entries}
    check(len(by_key) == len(entries), 'Duplicate settings')
    for e in entries:
        k, t, v, c = e['key'], e['type'], e['default'], e['constraints']
        check(e['scope'] in {'global','purpose','mapping','profile'}, k + ': scope')
        check(bool(e['consumer']) and len(e['description']) > 10, k + ': metadata')
        if t == 'boolean': check(type(v) is bool, k + ': type')
        elif t == 'integer': check(type(v) is int, k + ': type')
        elif t == 'number': check(type(v) in (int,float), k + ': type')
        elif t == 'enum': check(v in c['values'], k + ': enum')
        elif t == 'string': check(isinstance(v,str), k + ': type')
        elif t == 'string|null': check(v is None or isinstance(v,str), k + ': type')
        elif t == 'string[]': check(isinstance(v,list) and all(isinstance(x,str) for x in v) and len(v) == len(set(v)), k + ': array')
        elif t in {'field_map','task_map','override_map'}: check(isinstance(v,dict), k + ': map')
        else: check(False, k + ': unknown type')
        if 'min' in c: check(c['min'] <= v <= c['max'], k + ': range')
        if c.get('format') == 'nonempty' and v is not None: check(bool(v), k + ': empty')
    example = dict(flatten(tomllib.loads((ROOT / 'configuration/config.example.toml').read_text())))
    expected = {e['key'] for e in entries if '<' not in e['key'] and e['default'] is not None}
    check(set(example) == expected, 'TOML default coverage')
    for k,v in example.items(): check(k in by_key and v == by_key[k]['default'], k + ': TOML mismatch')
    files = sorted(ROOT.rglob('*.md'))
    routes = build_routes(files)
    if args.update_map:
        (ROOT / 'reading-map.json').write_text(json.dumps(routes,indent=2)+'\n')
    saved = json.loads((ROOT / 'reading-map.json').read_text())
    check(saved == routes, 'Stale reading map; run validate.py --update-map')
    ids = set(routes['records'])
    check({i for i in ids if i.startswith('OP-')} == {f'OP-{i:02}' for i in range(1,62)}, 'Operation coverage')
    check({i for i in ids if i.startswith('WP-')} == {f'WP-{i:02}' for i in range(1,24)}, 'Package coverage')
    algorithms = {i for i in ids if i.startswith('ALG-')}
    check(len(algorithms) == 18, 'Algorithm coverage')
    register = json.loads((ROOT/'decisions/register.json').read_text())
    check({r['id'] for r in register['decisions']} == {f'R{i:02}' for i in range(1,35)}, 'Final R decision coverage')
    check(all(r['status'] == 'resolved' for r in register['decisions']), 'Unresolved design decisions')
    gate_data = json.loads((ROOT/'decisions/release-gates.json').read_text())['gates']
    gate_ids = {g['id'] for g in gate_data}
    check(gate_ids == {f'EV-{i:02}' for i in range(1,15)}, 'Evidence gate coverage')
    check({g['id'] for g in register['gaps']} == {f'FG-{i:02}' for i in range(1,25)}, 'Final gap coverage')
    for r in register['decisions'] + register['gaps']:
        check((ROOT/'decisions'/r['document']).is_file(), 'Missing decision document: '+r['id'])
    for g in register['gaps']:
        check(g['status'] == 'approach_finalized' and g['owner'] in ids and g['evidence_gate'] in gate_ids, 'Unowned/unfinalized gap: '+g['id'])
    for g in gate_data:
        check(g['owners'] and all(owner in ids for owner in g['owners']), 'Invalid gate owner: '+g['id'])
        check(g['status'] in {'not_run','pass','fail','blocked'}, 'Invalid evidence status: '+g['id'])
        if g['status'] == 'pass':
            check(bool(g.get('evidence_paths')), 'Gate pass without evidence: '+g['id'])
            for path in g.get('evidence_paths',[]): check((REPO/path).is_file(), 'Missing gate evidence: '+path)
    presets = json.loads((ROOT/'configuration/purpose-defaults.json').read_text())['presets']
    check(set(presets) == {'japanese_vocab','japanese_grammar','english_vocab','english_grammar'}, 'Purpose preset coverage')
    for name, preset in presets.items():
        for key,value in preset['overrides'].items():
            check(key in by_key and by_key[key]['scope']=='purpose', name+': unregistered preset '+key)
            check(value is not None, name+': null preset '+key)
    text = '\n'.join(p.read_text() for p in files)
    refs = set(re.findall(r'\bALG-[A-Z]+\b', text))
    check(refs <= algorithms, 'Unknown algorithm references: ' + str(sorted(refs - algorithms)))
    for rec in routes['records'].values():
        for path in rec['requires']: check((ROOT/path).is_file(), 'Missing prerequisite: ' + path)
    for p in files + [REPO/'README.md', REPO/'legacy/README.md']:
        s = p.read_text()
        check(s.count('```') % 2 == 0, str(p.relative_to(REPO)) + ': fences')
        for target in re.findall(r'\]\(([^)]+)\)', s):
            if target.startswith(('http','mailto:')): continue
            name,sep,fragment = target.partition('#')
            path = (p.parent/name).resolve() if name else p
            check(path.exists(), str(p.relative_to(REPO)) + ': missing link ' + target)
            if sep and fragment and path.is_file() and path.suffix == '.md':
                headings = [slug(h) for h in re.findall(r'^#{1,6} (.+)$',path.read_text(),re.M)]
                check(fragment in headings, str(p.relative_to(REPO)) + ': missing anchor ' + target)
        for n,line in enumerate(s.splitlines(),1): check(line.rstrip() == line, f'{p.relative_to(REPO)}:{n}: whitespace')
    check(not any(p.name.startswith('cli-') for p in ROOT.parent.iterdir()), 'CLI documents outside docs/cli')
    for path in ['Cargo.toml','Cargo.lock','src','crates','tests','contracts','scripts','pyproject.toml','PKGBUILD','PKGBUILD.native']:
        check((REPO/'legacy'/path).exists(), 'Missing archived implementation: ' + path)
    if errors: raise SystemExit('\n'.join(errors))
    print(f'PASS: {len(entries)} settings/{len(index["groups"])} groups; 61 operations, 18 algorithms, 17 packages; {len(files)} Markdown files; routes, final decisions/gates/presets, links, defaults and legacy layout valid.')

if __name__ == '__main__': main()
