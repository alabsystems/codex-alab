#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Publish an exact fast-forward main through Agh without changing source guards."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys

SPEC = importlib.util.spec_from_file_location('alab_github', Path(__file__).with_name('github.py'))
GITHUB = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GITHUB)


class Advanced(GITHUB.Refused):
    pass


def is_ancestor(repo, before, after):
    # A missing object or command error is not a non-ancestor verdict.
    return GITHUB.git(repo, 'merge-base', before, after).decode().strip() == before


def _publish_local(transport, identity, after, expected):
    # The receiver enforces actual fast-forwards; the lease binds the old ref.
    receiver = shlex.join([
        'env', 'GIT_NO_REPLACE_OBJECTS=1', 'GIT_GRAFT_FILE=' + os.devnull,
        'git', '-c', 'receive.denyNonFastForwards=true', 'receive-pack',
    ])
    output = GITHUB.git(
        transport, 'push', '--porcelain', '--force-with-lease=refs/heads/main:' + expected,
        '--receive-pack=' + receiver, identity, after + ':refs/heads/main')
    # The destination is arbitrary path data, including status-shaped newlines.
    header = b'To ' + os.fsencode(identity) + b'\n'
    if not output.startswith(header):
        raise GITHUB.Refused('unexpected local publication response')
    lines = output[len(header):].split(b'\n')
    if len(lines) != 3 or lines[1:] != [b'Done', b'']:
        raise GITHUB.Refused('unexpected local publication response')
    row = lines[0].split(b'\t', 2)
    if len(row) != 3 or row[1] != (after + ':refs/heads/main').encode('ascii'):
        raise GITHUB.Refused('unexpected local publication response')
    flag = row[0]
    if flag == b'=' and expected != after:
        raise Advanced('main advanced to the candidate before local publication')
    if flag not in (b' ', b'='):
        raise GITHUB.Refused('local publication did not report a fast-forward update')
    return output


def publish(repo, remote, expected, *, client=None, admit_transfer=None):
    repo = Path(repo).resolve()
    if not GITHUB.OID.fullmatch(expected) or expected == GITHUB.ZERO:
        raise GITHUB.Refused('publication requires the existing main commit as a full object ID')
    after = GITHUB.git(repo, 'rev-parse', '--verify', 'HEAD^{commit}').decode().strip()
    if not is_ancestor(repo, expected, after):
        raise GITHUB.Refused('candidate does not fast-forward expected main')
    identity, network = GITHUB.location(repo, remote)
    if network:
        client = client or GITHUB.Agh()
    # Screen a fresh, complete transport without changing any source guards.
    # Preload only the candidate's graph locally; the admitted exact-main fetch
    # then transfers only missing objects, rather than mirroring every remote ref.
    with GITHUB.temporary_directory(prefix='alab-publish-', suffix='.noindex') as scratch:
        transport = Path(scratch) / 'transport.git'
        # The candidate's entire reachable history goes into this empty store.
        monitor = GITHUB.admit_transfer_monitor(admit_transfer, transport, None)
        GITHUB.git(scratch, 'init', '--bare', '--quiet', str(transport), failure_stage='publication-init')
        if monitor is not None:
            monitor()
        GITHUB.git(transport, 'fetch', '--no-tags', '--no-write-fetch-head',
                   '--recurse-submodules=no', str(repo), after + ':refs/alab/candidate',
                   failure_stage='publication-candidate-fetch', fetch_check=monitor)
        if not is_ancestor(transport, expected, after):
            raise GITHUB.Refused('canonical candidate does not fast-forward expected main')
        fetch_options = {'client': client, 'admit_transfer': admit_transfer}
        GITHUB.fetch(transport, identity,
                     ['refs/heads/main:refs/remotes/origin/main'], **fetch_options)
        current = GITHUB.git(transport, 'rev-parse', '--verify',
                             'refs/remotes/origin/main').decode().strip()
        if current != expected:
            if is_ancestor(transport, expected, current):
                raise Advanced('main advanced before publication')
            raise GITHUB.Refused('main changed history before publication')
        if not network:
            # Local fixture/offline repositories never serve as a GitHub fallback.
            return _publish_local(transport, identity, after, expected)
        # Bind the native request to the exact objects inspected above. Agh repeats
        # its ancestry and expected-ref checks, and owns all network writes.
        request = Path(scratch) / 'updates.json'
        request.write_text(json.dumps([dict(ref='refs/heads/main', before=expected, after=after)]) + '\n')
        try:
            return client.call('repo', 'push', identity, str(transport), '--input', str(request))
        except GITHUB.Refused as failure:
            native = failure.native_error or {}
            if not GITHUB.is_native_error(native, 9) or native.get('kind') != 'conflict':
                raise
            # A typed lease conflict alone is insufficient. Fetch fresh authoritative
            # main through the existing admitted helper and prove a forward advance.
            GITHUB.fetch(transport, identity, ['refs/heads/main:refs/remotes/origin/main'], **fetch_options)
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
        output = publish(args.repo, args.remote, args.expected,
                         admit_transfer=GITHUB.admit_release_transfer if os.name == 'posix' else None)
        sys.stdout.buffer.write(output)
    except SystemExit as error:
        if not GITHUB.successful_exit(error):
            raise
        return GITHUB.report_failure('ALab publication refused: ', error, incomplete=True)
    except Advanced as error:
        # Retain sync.sh's proven-race interface; all other failures remain refusals.
        return GITHUB.report_failure('', error,
                                     stdout_line='!\tHEAD:refs/heads/main\t[rejected] (fetch first)')
    except (GITHUB.Refused, OSError, subprocess.TimeoutExpired) as error:
        return GITHUB.report_transfer_failure('ALab publication refused: ', error)
    return 0


if __name__ == '__main__':
    GITHUB.restore_cli_sigchld()
    sys.exit(main())
