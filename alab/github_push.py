#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Publish an exact fast-forward main through Agh without changing source guards."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

SPEC = importlib.util.spec_from_file_location('alab_github', Path(__file__).with_name('github.py'))
GITHUB = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GITHUB)


class Advanced(GITHUB.Refused):
    pass


def is_ancestor(repo, before, after):
    # A missing object or command error is not a non-ancestor verdict.
    return GITHUB.git(repo, 'merge-base', before, after).decode().strip() == before


def publish(repo, remote, expected, *, client=None):
    repo = Path(repo).resolve()
    if not GITHUB.OID.fullmatch(expected) or expected == GITHUB.ZERO:
        raise GITHUB.Refused('publication requires the existing main commit as a full object ID')
    after = GITHUB.git(repo, 'rev-parse', '--verify', 'HEAD^{commit}').decode().strip()
    if not is_ancestor(repo, expected, after):
        raise GITHUB.Refused('candidate does not fast-forward expected main')
    identity, network = GITHUB.location(repo, remote)
    if not network:
        # Local repositories are fixture/offline transports, never a GitHub fallback.
        return GITHUB.git(repo, 'push', '--porcelain', identity, 'HEAD:refs/heads/main')
    client = client or GITHUB.Agh()
    # Agh explicitly recommends a fresh clone when source config cannot pass its
    # write screen. Preserve all source guards, and let Agh screen the new clone.
    with tempfile.TemporaryDirectory(prefix='alab-publish-', suffix='.noindex') as scratch:
        transport = Path(scratch) / 'transport.git'
        client.call('repo', 'clone', identity, str(transport), '--mirror')
        current = GITHUB.git(transport, 'rev-parse', '--verify', 'refs/heads/main').decode().strip()
        if current != expected:
            if is_ancestor(transport, expected, current):
                raise Advanced('main advanced before publication')
            raise GITHUB.Refused('main changed history before publication')
        GITHUB.git(transport, 'fetch', '--no-tags', '--no-write-fetch-head',
                   '--recurse-submodules=no', str(repo), after + ':refs/alab/candidate')
        # Bind the native request to the exact objects inspected above. Agh repeats
        # its ancestry and expected-ref checks, and owns all network writes.
        request = Path(scratch) / 'updates.json'
        request.write_text(json.dumps([dict(ref='refs/heads/main', before=expected, after=after)]) + '\n')
        try:
            return client.call('repo', 'push', identity, str(transport), '--input', str(request))
        except GITHUB.Refused as failure:
            native = failure.native_error or {}
            if native.get('v') != 1 or native.get('exit') != 9 or native.get('kind') != 'conflict' or native.get('side_effect') != 'none':
                raise
            # A typed lease conflict alone is insufficient. Fetch fresh authoritative
            # main through the existing admitted helper and prove a forward advance.
            GITHUB.fetch(transport, identity, ['refs/heads/main:refs/remotes/origin/main'], client=client)
            observed = GITHUB.git(transport, 'rev-parse', 'refs/remotes/origin/main').decode().strip()
            if observed != expected and is_ancestor(transport, expected, observed):
                raise Advanced('main advanced during publication') from failure
            raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', required=True, type=Path)
    parser.add_argument('--expected', required=True)
    parser.add_argument('remote')
    args = parser.parse_args()
    try:
        output = publish(args.repo, args.remote, args.expected)
        sys.stdout.buffer.write(output)
    except Advanced as error:
        # Retain sync.sh's proven-race interface; all other failures remain refusals.
        print('!\tHEAD:refs/heads/main\t[rejected] (fetch first)')
        print(str(error), file=sys.stderr)
        return 1
    except (GITHUB.Refused, OSError, subprocess.TimeoutExpired) as error:
        print('ALab publication refused: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
