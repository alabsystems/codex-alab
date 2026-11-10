#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Validate feature ownership of current hook topics (requires Python 3.11+)."""
import argparse
from pathlib import Path
import subprocess
import sys
import tomllib


def validate(catalog, topics, repo):
    errors, owners, ids = [], {}, set()
    if catalog.get('schema') != 1:
        errors.append('unsupported inventory schema')
    features = catalog.get('features')
    if not isinstance(features, list) or not features:
        return errors + ['features must be a nonempty list']
    for feature in features:
        if not isinstance(feature, dict):
            errors.append('feature must be a table')
            continue
        identity = feature.get('id')
        if not isinstance(identity, str) or not identity.strip():
            errors.append('feature ID required')
            continue
        if identity in ids:
            errors.append('duplicate feature ID: ' + identity)
        ids.add(identity)
        if feature.get('lifecycle') not in {'active', 'retiring', 'retired'}:
            errors.append(identity + ': invalid lifecycle')
        for field in ('purpose', 'equivalence'):
            if not isinstance(feature.get(field), str) or not feature[field].strip():
                errors.append(identity + ': ' + field + ' required')
        for field in ('topics', 'implementation', 'invariants', 'scenarios'):
            values = feature.get(field)
            if not isinstance(values, list) or not values or any(not isinstance(v, str) or not v.strip() for v in values):
                errors.append(identity + ': nonempty ' + field + ' required')
        for topic in feature.get('topics', []) if isinstance(feature.get('topics'), list) else []:
            if not isinstance(topic, str):
                continue
            if topic in owners:
                errors.append('duplicate topic owner: ' + topic)
            owners[topic] = identity
            if feature.get('lifecycle') == 'retired' and topic in topics:
                errors.append(identity + ': retired topic still has hooks: ' + topic)
        if feature.get('lifecycle') != 'retired':
            for field in ('implementation', 'policy_files'):
                for path in feature.get(field, []) if isinstance(feature.get(field, []), list) else []:
                    if not isinstance(path, str) or Path(path).is_absolute() or '..' in Path(path).parts or not (repo / path).exists():
                        errors.append(identity + ': invalid owned path ' + repr(path))
    for topic in sorted(topics - owners.keys()):
        errors.append('unowned hook topic: ' + topic)
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        with (args.repo / 'alab/features.toml').open('rb') as source:
            catalog = tomllib.load(source)
        output = subprocess.check_output([str(args.repo / 'alab/delta.sh'), '--topics'], cwd=args.repo, text=True)
        topics = {line for line in output.splitlines() if line and not line.startswith(' ')}
        errors = validate(catalog, topics, args.repo)
        if errors:
            print('\n'.join(errors), file=sys.stderr)
            return 1
        print(f"ownership complete: {len(topics)} hook topics, {len(catalog['features'])} features; semantic qualification is separate")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print('feature inventory: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
