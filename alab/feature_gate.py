#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Admit declared affected-feature evidence; full native qualification remains required."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from feature_evidence import snapshot, validate
from upstream_gate import admission as review_admission
from upstream_intake import git


def bundle_path(repo, source, override=None):
    if override is not None:
        path = Path(override)
        if not path.is_absolute():
            raise ValueError('feature bundle override must be absolute')
        return path
    common = Path(git(repo, 'rev-parse', '--path-format=absolute', '--git-common-dir'))
    return common / 'alab-feature-evidence' / source


def admission(repo, base, source='HEAD', upstream='upstream/main', bundle_root=None):
    review = review_admission(repo, base, source, upstream)
    if review['missing'] or review['errors']:
        raise ValueError('complete committed review required before feature admission')
    result = {'schema': 1, 'kind': 'declared-feature-evidence-admission',
              'source': review['source'], 'base': review['base'],
              'upstream': review['integrated_upstream'],
              'affected_features': review['affected_features'], 'bundle': None,
              'errors': [],
              'limits': 'Named execution declarations and reviewed impact scope only; '
                        'semantic judgment, native full gate and adoption remain separate.'}
    if not review['affected_features']:
        return result
    expected = snapshot(repo, review['source'], review['integrated_upstream'], review['base'])
    root = bundle_path(repo, review['source'], bundle_root)
    result['bundle'] = str(root)
    if root.is_symlink() or (root / 'record.json').is_symlink():
        raise ValueError('feature bundle root and record must not be symlinks')
    if not (root / 'record.json').is_file():
        result['errors'] = ['complete exact-source feature bundle required at ' + str(root)]
        return result
    record = json.loads((root / 'record.json').read_text())
    result['errors'] = validate(record, expected, root)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--base', required=True)
    parser.add_argument('--source', default='HEAD')
    parser.add_argument('--upstream', default='upstream/main')
    parser.add_argument('--bundle-root', type=Path, default=os.environ.get('ALAB_FEATURE_BUNDLE'))
    args = parser.parse_args()
    try:
        result = admission(args.repo, args.base, args.source, args.upstream, args.bundle_root)
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print('feature evidence admission: ' + str(error), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 1 if result['errors'] else 0


if __name__ == '__main__':
    sys.exit(main())
