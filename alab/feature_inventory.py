#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Validate feature ownership at one Git commit (requires Python 3.11+)."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from hook_topics import git_environment, read_topics, resolve_source


def validate(catalog, topics, repo, *, path_exists=None):
    errors, owners, ids = [], {}, set()
    if path_exists is None:
        path_exists = lambda path: (repo / path).exists()
    if type(catalog.get('schema')) is not int or catalog.get('schema') != 1:
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
        policy_files = feature.get('policy_files', [])
        if not isinstance(policy_files, list) or any(not isinstance(path, str) or not path.strip() for path in policy_files):
            errors.append(identity + ': policy_files must be a list of nonempty strings')
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
                    if not isinstance(path, str) or Path(path).is_absolute() or '..' in Path(path).parts or not path_exists(Path(path)):
                        errors.append(identity + ': invalid owned path ' + repr(path))
    for topic in sorted(topics - owners.keys()):
        errors.append('unowned hook topic: ' + topic)
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source', default='HEAD', help='commit or revision to inspect (default: HEAD)')
    args = parser.parse_args()
    try:
        repo = args.repo.resolve()
        env = git_environment()
        source = resolve_source(repo, args.source, env)
        catalog = tomllib.loads(subprocess.check_output(
            ['git', 'show', source + ':alab/features.toml'], cwd=repo, env=env, text=True))
        # Include directories and symlink entries, without following filesystem links
        # or letting untracked files satisfy a committed implementation path.
        entries = subprocess.check_output(
            ['git', 'ls-tree', '-r', '-t', '-z', '--name-only', '--full-tree', source], cwd=repo, env=env)
        paths = {Path('.')} | {Path(os.fsdecode(path)) for path in entries.split(b'\0') if path}
        topics = set(read_topics(repo, source, env))
        errors = validate(catalog, topics, repo, path_exists=paths.__contains__)
        if errors:
            print('feature inventory at ' + source + ':', file=sys.stderr)
            print('\n'.join(errors), file=sys.stderr)
            return 1
        print(f"ownership complete: {len(topics)} hook topics, {len(catalog['features'])} features at {source}; semantic qualification is separate")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print('feature inventory: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
