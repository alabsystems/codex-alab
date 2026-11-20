#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check declared feature/scenario evidence; does not run tests or prove semantics."""
import argparse
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile
import tomllib

if __name__ == '__main__':
    sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
# Preserve parser exports used by existing evidence collectors.
from focused_results import PASSED, RESULT, START, completed_names, confirm, libtest_output
from upstream_intake import git, upstream_target
from python_test_results import passed_tests as python_passed_tests

FILE_HASH = runpy.run_path(str(Path(__file__).resolve().with_name('file_hash.py')))
CLI = runpy.run_path(str(Path(__file__).resolve().with_name('cli_status.py')))
REQUIRED_TESTS = (('required_libtest_groups', 'required_libtests', 'libtest'),
                  ('required_python_groups', 'required_python_tests', 'python-unittest'))


def read_record(path):
    _metadata, raw = FILE_HASH['file_bytes_snapshot'](path)
    with io.TextIOWrapper(io.BytesIO(raw)) as text:
        return json.loads(text.read())


def snapshot(repo, source, upstream, base=None):
    source = git(repo, 'rev-parse', '--verify', '--end-of-options', source + '^{commit}')
    upstream = upstream_target(repo, source, upstream)
    upstream = git(repo, 'rev-parse', '--verify', '--end-of-options', upstream + '^{commit}')
    try:
        frontier = git(repo, 'merge-base', source, upstream)
    except subprocess.CalledProcessError as error:
        if error.returncode != 1:
            raise
        frontier = None
    if frontier != upstream:
        raise ValueError('source does not contain the declared upstream')
    raw = git(repo, 'show', source + ':alab/features.toml')
    catalog = tomllib.loads(raw)
    if type(catalog.get('schema')) is not int or catalog.get('schema') != 1:
        raise ValueError('unsupported feature inventory schema')
    features, ids = [], set()
    recipes = None
    for feature in catalog['features']:
        lifecycle = feature.get('lifecycle')
        if not isinstance(lifecycle, str) or lifecycle not in {'active', 'retiring', 'retired'}:
            raise ValueError('invalid feature lifecycle')
        if lifecycle == 'retired':
            continue
        identity = feature.get('id')
        if not isinstance(identity, str) or not identity.strip():
            raise ValueError('feature ID required')
        scenarios = feature.get('scenarios')
        if (not isinstance(scenarios, list) or not scenarios
                or any(not isinstance(description, str) or not description.strip()
                       for description in scenarios)):
            raise ValueError('nonempty scenario descriptions required')
        if identity in ids or len(set(scenarios)) != len(scenarios):
            raise ValueError('duplicate feature or scenario')
        ids.add(identity)
        rows = [{'description': scenario, 'checks': []} for scenario in scenarios]
        for group_field, row_field, result_format in REQUIRED_TESTS:
            if group_field not in feature:
                continue
            from feature_recipes.manifest import decode, required_tests
            if recipes is None:
                path = 'alab/feature_recipes/scenarios.json'
                recipes = decode(git(repo, 'show', source + ':' + path).encode(), path)
            names = required_tests(feature, recipes, group_field, result_format)
            for row in rows:
                if names[row['description']]:
                    row[row_field] = names[row['description']]
        features.append({'id': identity, 'scenarios': rows})
    if not features:
        raise ValueError('no active feature contracts')
    result = {'schema': 1, 'source': source, 'upstream': upstream,
              'inventory_sha256': hashlib.sha256(raw.encode()).hexdigest(),
              'features': features}
    if base is None:
        return result
    from upstream_gate import admission
    return scoped_snapshot(result, admission(repo, base, source, upstream))


def scoped_snapshot(inventory, review):
    """Apply one committed review to the inventory of the same immutable candidate."""
    if review['missing'] or review['errors']:
        raise ValueError('complete committed upstream review required before feature scoping')
    if (inventory['source'] != review['source']
            or inventory['upstream'] != review['integrated_upstream']):
        raise ValueError('committed review differs from feature inventory source or frontier')
    affected = set(review['affected_features'])
    features = [feature for feature in inventory['features'] if feature['id'] in affected]
    if len(features) != len(affected):
        raise ValueError('affected feature has no active or retiring contract')
    return {**inventory, 'base': review['base'], 'features': features}


def log_bytes(observation, log_root=None):
    log = observation.get('log')
    if log_root is None:
        if not isinstance(log, str) or not Path(log).is_absolute():
            raise ValueError('absolute captured log path required')
        try:
            path = Path(log).resolve(strict=True)
        except RuntimeError as error:
            raise ValueError('captured log path cannot be resolved') from error
    else:
        digest = observation.get('log_sha256')
        if (not isinstance(digest, str) or not re.fullmatch(r'[a-f0-9]{64}', digest)
                or log != 'logs/' + digest + '.log'):
            raise ValueError('bundle log must use its content-addressed relative path')
        root = Path(log_root).resolve(strict=True)
        path = root / log
        if (path.is_symlink() or path.parent.is_symlink()
                or not path.resolve(strict=True).is_relative_to(root)):
            raise ValueError('bundle log must remain inside the bundle without symlinks')
    _metadata, raw = FILE_HASH['file_bytes_snapshot'](path)
    if hashlib.sha256(raw).hexdigest() != observation.get('log_sha256'):
        raise ValueError('captured log hash changed')
    return raw


def check(observation, source, log_root=None):
    if not isinstance(observation, dict):
        raise ValueError('check must be an object')
    if observation.get('source') != source:
        raise ValueError('check source differs from candidate')
    if type(observation.get('status')) is not int or observation['status'] != 0:
        raise ValueError('check requires an actual successful command status')
    command = observation.get('command')
    if not isinstance(command, list) or not command or any(
            not isinstance(arg, str) or not arg or '\0' in arg for arg in command):
        raise ValueError('command must be a nonempty argument list')
    reason = observation.get('reason')
    if not isinstance(reason, str) or not reason.strip():
        raise ValueError('explain how these tests cover this scenario')
    tests = observation.get('tests')
    if (not isinstance(tests, list) or not tests
            or any(not isinstance(test, str) or not test for test in tests)
            or len(set(tests)) != len(tests)):
        raise ValueError('unique executed test names required')
    raw = log_bytes(observation, log_root)
    if observation.get('format', 'libtest') == 'python-unittest':
        passed = python_passed_tests(json.loads(raw))
        if any(passed.count(test) != 1 for test in tests):
            raise ValueError('selected Python test is missing, duplicated or did not pass')
        return
    if observation.get('format', 'libtest') != 'libtest':
        raise ValueError('unknown test result format')
    output = libtest_output(raw)
    suites = confirm(output, observation['status'], 1)
    passed = completed_names(output, suites)
    if any(passed.count(test) != 1 for test in tests):
        raise ValueError('selected test is missing, duplicated or did not pass')


def validate(record, expected, log_root=None):
    return _validate_record(record, expected, log_root, allow_pending=False)


def missing_required_tests(scenario, passed):
    for _, row_field, result_format in REQUIRED_TESTS:
        missing = [test for test in scenario.get(row_field, ()) if (result_format, test) not in passed]
        if missing:
            yield row_field, missing


def _validate_record(record, expected, log_root, *, allow_pending):
    errors = []
    if not isinstance(record, dict):
        return ['record must be an object']
    if 'base' in record and 'base' not in expected:
        errors.append('scoped evidence requires its original base')
    fields = ('schema', 'source', 'upstream', 'inventory_sha256')
    if 'base' in expected:
        fields += ('base',)
    for field in fields:
        if (record.get(field) != expected[field]
                or field == 'schema' and type(record.get(field)) is not int):
            errors.append('stale or invalid ' + field)
    features = record.get('features')
    if (not isinstance(features, list) or any(not isinstance(f, dict) for f in features)
            or [f.get('id') for f in features] != [f['id'] for f in expected['features']]):
        return errors + ['feature coverage differs from inventory']
    for feature, contract in zip(features, expected['features']):
        prefix = feature['id'] + ': '
        scenarios = feature.get('scenarios')
        if (not isinstance(scenarios, list) or any(not isinstance(s, dict) for s in scenarios)
                or [s.get('description') for s in scenarios] != [
                    s['description'] for s in contract['scenarios']]):
            errors.append(prefix + 'scenario coverage differs from inventory')
            continue
        for scenario, required in zip(scenarios, contract['scenarios'], strict=True):
            label = prefix + scenario['description'] + ': '
            checks = scenario.get('checks')
            if not isinstance(checks, list) or (not checks and not allow_pending):
                errors.append(label + 'executed evidence required')
                continue
            passed = set()
            for observation in checks:
                try:
                    check(observation, expected['source'], log_root)
                except (OSError, UnicodeError, ValueError) as error:
                    errors.append(label + str(error))
                else:
                    result_format = observation.get('format', 'libtest')
                    passed.update((result_format, test) for test in observation['tests'])
            if not allow_pending:
                for row_field, missing in missing_required_tests(required, passed):
                    errors.append(label + 'missing ' + row_field.replace('_', ' ') + ': ' + ', '.join(missing))
    return errors


def combine(records, expected, *, first_log_root=None):
    """Validate and combine one snapshot's records. Only the first may use an explicit portable log root."""
    result = copy.deepcopy(expected)
    for index, record in enumerate(records):
        errors = _validate_record(record, expected, first_log_root if index == 0 else None, allow_pending=True)
        if errors:
            raise ValueError('\n'.join(errors))
        for source, target in zip(record['features'], result['features'], strict=True):
            for part, scenario in zip(source['scenarios'], target['scenarios'], strict=True):
                checks = scenario['checks']
                for observation in part['checks']:
                    # Missing and explicit libtest identify the same format. Retain the first declaration.
                    identity = {'format': 'libtest', **observation}
                    if not any({'format': 'libtest', **previous} == identity for previous in checks):
                        checks.append(observation)
    return result


def progress(records, expected, log_root=None):
    return _progress_report(combine(records, expected, first_log_root=log_root))


def progress_summary(report):
    features = report['features']
    scope = 'affected' if report['scope'] == 'affected-features' else 'inventory'
    contracts = 'feature contract' if len(features) == 1 else 'feature contracts'
    lines = [f"ALab release scenario coverage for {report['source']}",
             f"{report['covered']} of {report['total']} verified across {len(features)} {scope} {contracts}",
             f"Pending scenarios   {report['total'] - report['covered']}"]
    for feature in features:
        if feature['pending']:
            numbers = ', '.join(str(scenario['number']) for scenario in feature['pending'])
            lines.append(f"  {feature['id']}   {feature['covered']} of {feature['total']} verified, pending {numbers}")
    return '\n'.join(lines)


def _progress_report(observed):
    features = []
    for feature in observed['features']:
        pending = []
        for number, scenario in enumerate(feature['scenarios'], 1):
            passed = {(observation.get('format', 'libtest'), test)
                      for observation in scenario['checks'] for test in observation['tests']}
            missing = dict(missing_required_tests(scenario, passed))
            if not passed or missing:
                row = {'number': number, 'description': scenario['description']}
                if missing:
                    row['missing'] = missing
                pending.append(row)
        total = len(feature['scenarios'])
        features.append({'id': feature['id'], 'covered': total - len(pending), 'total': total, 'pending': pending})
    return {**{key: value for key, value in observed.items() if key != 'features'},
            'scope': 'affected-features' if 'base' in observed else 'inventory',
            'covered': sum(f['covered'] for f in features), 'total': sum(f['total'] for f in features),
            'features': features, 'limits': 'Counts verified named scenario evidence only. '
                                           'Semantic review and release qualification have separate gates.'}


def require_valid(record, expected, log_root=None):
    errors = validate(record, expected, log_root)
    if errors:
        raise ValueError('\n'.join(errors))


def stage_bundle_logs(record, expected, destination, *, retained_logs=None):
    """Stage missing captures and return the record to validate at its final log root."""
    require_valid(record, expected)
    return _copy_captures(record, destination, retained_logs=retained_logs)


def _copy_captures(record, destination, *, portable_root=None, retained_logs=None):
    # Create exclusively; a resumed export must never overwrite an earlier record.
    destination = Path(destination)
    destination.mkdir()
    # Failed exports retain their material. The pathname can have been replaced
    # since creation, so recursive rollback cannot establish deletion custody.
    (destination / 'logs').mkdir()
    # JSON round-tripping copies only the record's serializable values.
    exported = json.loads(json.dumps(record))
    for feature in exported['features']:
        for scenario in feature['scenarios']:
            for observation in scenario['checks']:
                # combine() admits portable paths only in the first input record.
                source_root = None if Path(observation['log']).is_absolute() else portable_root
                raw = log_bytes(observation, source_root)
                observation['log'] = 'logs/' + observation['log_sha256'] + '.log'
                target = destination / observation['log']
                if retained_logs is not None and os.path.lexists(retained_logs / target.name):
                    continue
                if not target.exists():
                    with target.open('xb') as stream:
                        stream.write(raw)
    return exported


def bundle(record, expected, destination):
    destination = Path(destination)
    exported = stage_bundle_logs(record, expected, destination)
    require_valid(exported, expected, destination)
    with (destination / 'record.json').open('x') as stream:
        json.dump(exported, stream, indent=2)
        stream.write('\n')


def export_progress(records, expected, destination, log_root=None):
    combined = combine(records, expected, first_log_root=log_root)
    destination = Path(destination)
    copied = _copy_captures(combined, destination, portable_root=log_root)
    # The same observation from portable and local inputs now has one log path.
    exported = combine([copied], expected, first_log_root=destination)
    report = _progress_report(exported)
    # Expose the final record only after the complete JSON has been closed.
    with tempfile.TemporaryDirectory(dir=destination) as staging:
        record = Path(staging) / 'progress-record.json'
        record.write_text(json.dumps(exported, indent=2) + '\n', encoding='utf-8')
        os.link(record, destination / record.name)
    return report


def _arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['snapshot', 'validate', 'bundle', 'progress', 'export-progress'])
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source', required=True)
    parser.add_argument('--upstream', required=True)
    parser.add_argument('--base', help='scope to affected features from committed upstream reviews')
    parser.add_argument('--record', type=Path, required=True)
    parser.add_argument('--summary', action='store_true', help='show progress as counts and pending scenario numbers')
    parser.add_argument('--part', type=Path, action='append', default=[],
                        help='add a local record of the same scope for bundle, progress or export-progress (repeatable)')
    parser.add_argument('--log-root', type=Path,
                        help='validate content-addressed logs for --record inside this portable bundle')
    parser.add_argument('--destination', type=Path, help='new directory for bundle or export-progress')
    args = parser.parse_args()
    if args.summary and args.operation != 'progress':
        parser.error('--summary requires progress')
    if args.part and args.operation not in ('bundle', 'progress', 'export-progress'):
        parser.error('--part requires bundle, progress or export-progress')
    if args.log_root is not None and args.operation not in ('validate', 'progress', 'export-progress'):
        parser.error('--log-root requires validate, progress or export-progress')
    if (args.destination is not None) != (args.operation in ('bundle', 'export-progress')):
        parser.error('bundle and export-progress require --destination and only those operations accept it')
    return args


def main():
    return _run(_arguments())


def _run(args):
    try:
        expected = snapshot(args.repo, args.source, args.upstream, args.base)
        if args.operation == 'snapshot':
            with args.record.open('x') as stream:
                json.dump(expected, stream, indent=2)
                stream.write('\n')
            print('Created pending feature coverage record; no scenarios qualified')
            return 0
        try:
            paths = [path.resolve(strict=True) for path in [args.record, *args.part]]
        except RuntimeError as error:
            raise ValueError('feature record path cannot be resolved') from error
        records = [read_record(path) for path in paths]
        if args.operation == 'export-progress':
            print(json.dumps(export_progress(records, expected, args.destination, args.log_root), indent=2))
            return 0
        if args.operation == 'progress':
            report = progress(records, expected, args.log_root)
            print(progress_summary(report) if args.summary else json.dumps(report, indent=2))
            return 0
        record = combine(records, expected) if args.part else records[0]
        if args.operation == 'bundle':
            bundle(record, expected, args.destination)
            print('Exported declared scenario evidence and hashed logs; no tests were run')
            return 0
        errors = validate(record, expected, args.log_root)
        if errors:
            print('\n'.join(errors), file=sys.stderr)
            return 1
        print('Declared scenario evidence is complete; semantic review and full gate remain separate')
        return 0
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print('feature evidence: ' + str(error), file=sys.stderr)
        return 1


def cli():
    args = _arguments()
    return CLI['run_operation'](lambda: _run(args), 'feature evidence: ')


if __name__ == '__main__':
    sys.exit(cli())
