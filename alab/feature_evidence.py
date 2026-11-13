#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check declared feature/scenario evidence; does not run tests or prove semantics."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))
# Preserve parser exports used by existing evidence collectors.
from focused_results import PASSED, RESULT, START, completed_names, confirm, libtest_output
from upstream_intake import git
from python_test_results import passed_tests as python_passed_tests


def snapshot(repo, source, upstream, base=None):
    source = git(repo, 'rev-parse', '--verify', '--end-of-options', source + '^{commit}')
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
    for feature in catalog['features']:
        if feature['lifecycle'] == 'retired':
            continue
        identity = feature['id']
        scenarios = feature['scenarios']
        if identity in ids or not scenarios or len(set(scenarios)) != len(scenarios):
            raise ValueError('duplicate feature or missing/duplicate scenario')
        ids.add(identity)
        features.append({'id': identity, 'scenarios': [
            {'description': scenario, 'checks': []} for scenario in scenarios]})
    if not features:
        raise ValueError('no active feature contracts')
    result = {'schema': 1, 'source': source, 'upstream': upstream,
              'inventory_sha256': hashlib.sha256(raw.encode()).hexdigest(),
              'features': features}
    if base is not None:
        from upstream_gate import admission
        review = admission(repo, base, source, upstream)
        if review['missing'] or review['errors']:
            raise ValueError('complete committed upstream review required before feature scoping')
        affected = set(review['affected_features'])
        if not affected <= ids:
            raise ValueError('affected feature has no active or retiring contract')
        result['base'] = review['base']
        result['features'] = [feature for feature in features if feature['id'] in affected]
    return result


def log_bytes(observation, log_root=None):
    log = observation.get('log')
    if log_root is None:
        if not isinstance(log, str) or not Path(log).is_absolute():
            raise ValueError('absolute captured log path required')
        path = Path(log)
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
    raw = path.read_bytes()
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
            not isinstance(arg, str) or not arg for arg in command):
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
    errors = []
    if not isinstance(record, dict):
        return ['record must be an object']
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
        for scenario in scenarios:
            label = prefix + scenario['description'] + ': '
            checks = scenario.get('checks')
            if not isinstance(checks, list) or not checks:
                errors.append(label + 'executed evidence required')
                continue
            for observation in checks:
                try:
                    check(observation, expected['source'], log_root)
                except (OSError, UnicodeError, ValueError) as error:
                    errors.append(label + str(error))
    return errors


def bundle(record, expected, destination):
    errors = validate(record, expected)
    if errors:
        raise ValueError('\n'.join(errors))
    # Create exclusively; a resumed export must never overwrite an earlier record.
    destination = Path(destination)
    destination.mkdir()
    try:
        (destination / 'logs').mkdir()
        # JSON round-tripping copies only the record's serializable values.
        exported = json.loads(json.dumps(record))
        for feature in exported['features']:
            for scenario in feature['scenarios']:
                for observation in scenario['checks']:
                    raw = log_bytes(observation)
                    observation['log'] = 'logs/' + observation['log_sha256'] + '.log'
                    target = destination / observation['log']
                    if not target.exists():
                        with target.open('xb') as stream:
                            stream.write(raw)
        errors = validate(exported, expected, destination)
        if errors:
            raise ValueError('\n'.join(errors))
        with (destination / 'record.json').open('x') as stream:
            json.dump(exported, stream, indent=2)
            stream.write('\n')
    except BaseException:
        shutil.rmtree(destination)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['snapshot', 'validate', 'bundle'])
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source', required=True)
    parser.add_argument('--upstream', required=True)
    parser.add_argument('--base', help='scope to affected features from committed upstream reviews')
    parser.add_argument('--record', type=Path, required=True)
    parser.add_argument('--log-root', type=Path,
                        help='validate content-addressed logs inside this portable bundle')
    parser.add_argument('--destination', type=Path, help='new directory for bundle export')
    args = parser.parse_args()
    if (args.log_root is not None and args.operation != 'validate'
            or (args.destination is not None) != (args.operation == 'bundle')):
        parser.error('--log-root requires validate; --destination is required only for bundle')
    try:
        expected = snapshot(args.repo, args.source, args.upstream, args.base)
        if args.operation == 'snapshot':
            with args.record.open('x') as stream:
                json.dump(expected, stream, indent=2)
                stream.write('\n')
            print('Created pending feature coverage record; no scenarios qualified')
            return 0
        record = json.loads(args.record.read_text())
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


if __name__ == '__main__':
    sys.exit(main())
