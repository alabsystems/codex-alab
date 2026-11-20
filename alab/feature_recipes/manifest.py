# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read qualification recipes and merge independent named entries for a sync."""
import json
from pathlib import Path
import subprocess
import sys


RUST = ('websocket', 'transport', 'state', 'aterm', 'recovery', 'core', 'tui', 'autopilot-integration', 'autopilot-turn',
        'boot', 'core-integration', 'app-server', 'exec', 'alab-tui', 'kimi-library', 'fleet-cli',
        'login-reader', 'cloud-reader', 'autopilot-library', 'goal-extension',
        'models-manager', 'api-fixtures', 'watch-fixtures', 'tui-fixtures', 'tool-recovery-cli', 'queue-review')
PYTHON = ('proof_build', 'package-inputs', 'qualification', 'upstream_gate', 'feature_gate', 'promote',
          'package_version', 'version_lines', 'hook-controls', 'check_runtime', 'reset_notice_oracle')
WIRE = ('wire', 'model-wire', 'product-wire', 'skill-wire', 'native-ui-wire', 'execution-wire')
NATIVE_PYTHON = (*WIRE, 'unit-comparison')
STAGE_FORMATS = {**dict.fromkeys(PYTHON, 'python-unittest'), **dict.fromkeys(RUST, 'libtest'),
                 **dict.fromkeys(NATIVE_PYTHON, 'python-unittest')}


def test_names(group, name):
    tests = group['tests']
    prefix = group.get('prefix', '')
    if (not isinstance(tests, list) or not tests or not isinstance(prefix, str)
            or any(not isinstance(t, str) or not t or t.startswith('-') for t in tests)):
        raise ValueError('invalid test selection for ' + name)
    return [prefix + test for test in tests]


def selected_test_names(group, name, scenario):
    names = test_names(group, name)
    if 'select_if_required' not in group:
        return names
    # A newer collector can offer a case that an older candidate never shipped.
    # Only the immutable candidate scenario declares whether it needs that case.
    field = group['select_if_required']
    if field != 'required_libtests' or STAGE_FORMATS[group['stage']] != 'libtest':
        raise ValueError('invalid source requirement for ' + name)
    required = scenario.get(field, [])
    if (not isinstance(required, list) or any(not isinstance(n, str) or not n.strip() for n in required)
            or len(set(required)) != len(required)):
        raise ValueError('invalid ' + field + ' for ' + scenario['description'])
    return [test for test in names if test in required]


def required_libtests(feature, recipes):
    return required_tests(feature, recipes, 'required_libtest_groups', 'libtest')


def required_tests(feature, recipes, group_field, result_format):
    names = feature[group_field]
    if (not isinstance(names, list) or not names or any(not isinstance(n, str) or not n for n in names)
            or len(set(names)) != len(names)):
        raise ValueError(f'invalid {group_field} for ' + feature['id'])
    required = {}
    for name in names:
        group = recipes['groups'].get(name)
        if not isinstance(group, dict) or STAGE_FORMATS.get(group.get('stage')) != result_format:
            raise ValueError(f'missing or non-{result_format} required group: ' + name)
        required[name] = test_names(group, name)
    rows = recipes['features'][feature['id']]
    if [row['scenario'] for row in rows] != feature['scenarios']:
        raise ValueError('scenario contract changed for ' + feature['id'])
    used = set()
    by_scenario = {}
    for row in rows:
        if not isinstance(row['groups'], list) or not all(isinstance(n, str) for n in row['groups']):
            raise ValueError('invalid recipe groups for ' + row['scenario'])
        selected = required.keys() & row['groups']
        used.update(selected)
        by_scenario[row['scenario']] = sorted({test for name in selected for test in required[name]})
    if used != required.keys():
        raise ValueError(f'required {result_format} group has no scenario: ' + ', '.join(sorted(required.keys() - used)))
    return by_scenario


def decode(raw, name):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f'{name}: duplicate feature recipe key {key!r}')
            result[key] = value
        return result

    return json.loads(raw.decode('utf-8'), object_pairs_hook=unique)


def source_for_key(key, base, current, incoming):
    # None denotes absence. JSON null and all other JSON types retain their encoding.
    old, ours, theirs = (json.dumps(doc[key], sort_keys=True, allow_nan=False) if key in doc else None
                         for doc in (base, current, incoming))
    if ours == theirs or theirs == old:
        return current
    if ours == old:
        return incoming
    return None


def merge_scenarios(feature, base, current, incoming):
    conflict = f'concurrent changes to features.{feature}'
    indexed = []
    for rows in (base, current, incoming):
        if not isinstance(rows, list):
            raise ValueError(conflict)
        by_name = {}
        for row in rows:
            name = row.get('scenario') if isinstance(row, dict) else None
            if not isinstance(name, str) or not name.strip() or name in by_name:
                raise ValueError(conflict + ': missing or duplicate scenario identity')
            by_name[name] = row
        indexed.append(by_name)

    old, ours, theirs = map(list, indexed)
    if ours == theirs or theirs == old:
        order = ours
    elif ours == old:
        order = theirs
    else:
        raise ValueError(conflict + ': incompatible scenario order')
    merged = []
    # Inspect removed names too, so deleting a row cannot discard new evidence.
    for name in dict.fromkeys([*order, *old]):
        source = source_for_key(name, *indexed)
        if source is None:
            raise ValueError(f'{conflict}: {name}')
        if name in source:
            merged.append(source[name])
    return merged


def merge(base, current, incoming):
    sections = {'features', 'groups'}
    for document in (base, current, incoming):
        if not isinstance(document, dict) or document.keys() != sections or not all(
                isinstance(entries, dict) for entries in document.values()):
            raise ValueError('unknown feature recipe structure')

    merged = {}
    for section in incoming:
        before, left, right = (d[section] for d in (base, current, incoming))
        entries = merged[section] = {}
        # Keep incoming main's layout and append only entries new to the candidate.
        for key in dict.fromkeys([*right, *left]):
            selected = source_for_key(key, before, left, right)
            if selected is None:
                if section == 'features' and key in before and key in left and key in right:
                    entries[key] = merge_scenarios(key, before[key], left[key], right[key])
                    continue
                raise ValueError(f'concurrent changes to {section}.{key}')
            if key in selected:
                entries[key] = selected[key]
    for feature, rows in merged['features'].items():
        if not isinstance(rows, list):
            raise ValueError(f'unknown recipe structure for {feature}')
        for row in rows:
            groups = row.get('groups') if isinstance(row, dict) else None
            if not isinstance(groups, list) or not all(isinstance(key, str) for key in groups):
                raise ValueError(f'unknown group references for {feature}')
            for key in groups:
                if key not in merged['groups']:
                    raise ValueError(f'{feature} references missing group {key}')
    return merged


def run(base, current, incoming, marker_size):
    paths = (base, current, incoming)
    raw = [p.read_bytes() for p in paths]
    try:
        result = merge(*(decode(data, path.name) for data, path in zip(raw, paths)))
    except ValueError as error:
        print(f'feature recipe merge: {error}', file=sys.stderr)
        # Give the resolver normal Git conflict context. A text merge can look
        # clean even when two overlapping edits changed the same named entry.
        text = subprocess.run(['git', 'merge-file', '-p', '--diff3', '--marker-size', str(marker_size),
                               '-L', 'current', '-L', 'base', '-L', 'incoming', current, base, incoming],
                              stdout=subprocess.PIPE)
        if 0 < text.returncode < 128:
            current.write_bytes(text.stdout)
        else:
            ancestor, ours, theirs = (data.removesuffix(b'\n') + b'\n' for data in raw)
            current.write_bytes(b'<' * marker_size + b' current\n' + ours
                                + b'|' * marker_size + b' base\n' + ancestor
                                + b'=' * marker_size + b'\n' + theirs
                                + b'>' * marker_size + b' incoming\n')
        return 1
    current.write_text(json.dumps(result, indent=2, ensure_ascii=False, allow_nan=False) + '\n', encoding='utf-8')
    return 0


if __name__ == '__main__':
    sys.exit(run(*(Path(arg) for arg in sys.argv[1:4]), int(sys.argv[4])))
