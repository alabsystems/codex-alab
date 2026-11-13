#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read exact hook identities from one committed tree; format only at the edge."""
import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess
import sys


# Match lib.sh's ALAB_MARKER_RE: every nonempty topic counts, including whitespace.
MARKER_RE = r'ALAB(\([^)]+\))?:'
MARKER = re.compile(rb'ALAB(?:\(([^)]+)\))?:')


@dataclass(frozen=True)
class Hook:
    path: str
    line: int
    text: str


def git_environment():
    env = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
    env.update(GIT_NO_LAZY_FETCH='1', GIT_NO_REPLACE_OBJECTS='1',
               GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
    return env


def resolve_source(repo, revision, env):
    return subprocess.check_output(
        ['git', 'rev-parse', '--verify', '--end-of-options', revision + '^{commit}'],
        cwd=repo, env=env).decode('ascii').strip()


def read_topics(repo, source, env):
    command = ['git', '-c', 'core.quotepath=off', 'grep', '--color=never',
               '--no-heading', '--no-break', '--no-column', '--full-name',
               '--text', '--null', '-n', '-E', MARKER_RE, source, '--',
               ':!alab', ':!AGENTS.md', ':!codex-rs/alab-*']
    result = subprocess.run(command, cwd=repo, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE)
    if result.returncode not in (0, 1):
        raise subprocess.CalledProcessError(result.returncode, command,
                                            output=result.stdout, stderr=result.stderr)
    if result.returncode == 1:
        if result.stdout:
            raise ValueError('unexpected hook output for a no-match result')
        return {}
    if not result.stdout:
        raise ValueError('missing hook output for a successful match result')

    topics = {}
    prefix = source.encode('ascii') + b':'
    offset = 0
    while offset < len(result.stdout):
        path_end = result.stdout.find(b'\0', offset)
        number_end = result.stdout.find(b'\0', path_end + 1) if path_end >= 0 else -1
        text_end = result.stdout.find(b'\n', number_end + 1) if number_end >= 0 else -1
        if min(path_end, number_end, text_end) < 0:
            raise ValueError('incomplete hook record from git grep')
        path = result.stdout[offset:path_end]
        number = result.stdout[path_end + 1:number_end]
        text = result.stdout[number_end + 1:text_end]
        offset = text_end + 1
        if not path.startswith(prefix) or not number.isdigit() or int(number) < 1:
            raise ValueError('invalid hook location from git grep')
        hook = Hook(os.fsdecode(path[len(prefix):]), int(number), os.fsdecode(text))
        found = {match.group(1) if match.group(1) is not None else b'(untagged)'
                 for match in MARKER.finditer(text)}
        if not found:
            raise ValueError('hook record contains no ALAB marker')
        for topic in found:
            topics.setdefault(os.fsdecode(topic), []).append(hook)
    return topics


def display(text, *, source_line=False):
    controls = any((ord(char) < 32 and not (source_line and char == '\t'))
                   or 127 <= ord(char) < 160 or char in '\u2028\u2029' for char in text)
    if controls or (not source_line and text != text.strip()):
        return json.dumps(text, ensure_ascii=True)
    return text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--source', default='HEAD')
    args = parser.parse_args()
    try:
        repo = args.repo.resolve()
        env = git_environment()
        source = resolve_source(repo, args.source, env)
        topics = read_topics(repo, source, env)
        lines = []
        for topic in sorted(topics):
            lines.append(display(topic))
            lines.extend(f'  {display(hook.path)}:{hook.line}:{display(hook.text, source_line=True)}'
                         for hook in topics[topic])
        if lines:
            sys.stdout.buffer.write(('\n'.join(lines) + '\n').encode('utf-8', errors='surrogateescape'))
    except subprocess.CalledProcessError as error:
        if error.stderr:
            sys.stderr.buffer.write(error.stderr)
        print('hook topics: ' + str(error), file=sys.stderr)
        return error.returncode if error.returncode > 0 else 1
    except (OSError, ValueError) as error:
        print('hook topics: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
