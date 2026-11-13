#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Inspect explicit local artifact/process identities without running the artifact."""
import hashlib
from contextlib import ExitStack
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

from promote import file_identity, regular_fd_bytes


def package_identity(directory):
    root = Path(directory).resolve(strict=True)
    manifest = root / 'alab-build.json'
    image = root / 'bin/codex'
    with ExitStack() as opened:
        flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC

        def hold(path, parent=None, directory=False):
            fd = os.open(path, flags | (os.O_DIRECTORY if directory else 0), dir_fd=parent)
            opened.callback(os.close, fd)
            return fd, os.fstat(fd)

        root_fd, root_before = hold(root, directory=True)
        bin_fd, bin_before = hold('bin', root_fd, directory=True)
        manifest_fd, manifest_before = hold('alab-build.json', root_fd)
        image_fd, metadata = hold('codex', bin_fd)
        if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
            raise ValueError('artifact executable must remain a regular executable inside its package')
        raw = regular_fd_bytes(manifest_fd, manifest)
        value = json.loads(raw)
        if (not isinstance(value, dict) or not isinstance(value.get('source'), str)
                or not re.fullmatch(r'(?:[0-9a-f]{40}|[0-9a-f]{64})', value['source'])
                or not isinstance(value.get('version'), str) or not value['version']):
            raise ValueError('artifact manifest needs a full source identity and version')
        for fd, before, name, parent in (
                (root_fd, root_before, root, None), (bin_fd, bin_before, 'bin', root_fd),
                (manifest_fd, manifest_before, 'alab-build.json', root_fd),
                (image_fd, metadata, 'codex', bin_fd)):
            named = os.stat(name, dir_fd=parent, follow_symlinks=False)
            if (file_identity(before) != file_identity(os.fstat(fd))
                    or file_identity(before) != file_identity(named)):
                raise ValueError('artifact changed during inspection')
        return {'source': value['source'], 'version': value['version'],
                'package': str(root), 'image': str(image),
                'manifest_sha256': hashlib.sha256(raw).hexdigest(),
                'image_device': metadata.st_dev, 'image_inode': metadata.st_ino,
                'identity_basis': 'packaged manifest; source qualification is not inferred'}


def executable_identity(executable):
    image = Path(executable).resolve(strict=True)
    if image.name != 'codex' or image.parent.name != 'bin':
        raise ValueError('executable does not resolve to a packaged bin/codex')
    result = package_identity(image.parent.parent)
    if result['image'] != str(image):
        raise ValueError('executable differs from the package image')
    result['executable_pointer'] = str(executable)
    return result


def mapped_images(output, pid):
    """Read exactly one requested process and its newline-framed txt records."""
    if not output.endswith('\n'):
        raise ValueError('incomplete loaded-image records')
    images, current, process_seen = [], None, False

    def finish_file():
        if current is not None:
            if 'path' not in current:
                raise ValueError('loaded-image record has no path')
            images.append(current)

    for line in output.split('\n')[:-1]:
        field, value = line[:1], line[1:]
        if field == 'p':
            if process_seen or value != str(pid):
                raise ValueError('loaded-image process differs from the requested PID')
            process_seen = True
        elif field == 'f':
            if not process_seen or value != 'txt':
                raise ValueError('invalid loaded-image file descriptor')
            finish_file()
            current = {}
        elif field in ('D', 'i', 'n'):
            if current is None:
                raise ValueError('loaded-image field has no file record')
            name = {'D': 'device', 'i': 'inode', 'n': 'path'}[field]
            if name in current:
                raise ValueError('duplicate loaded-image field')
            if field == 'i' and re.fullmatch(r'[0-9]+', value) is None:
                raise ValueError('invalid loaded-image inode')
            if field == 'D' and re.fullmatch(r'0x[0-9a-fA-F]+', value) is None:
                raise ValueError('invalid loaded-image device')
            if field == 'n' and (not Path(value).is_absolute() or '\0' in value):
                raise ValueError('invalid loaded-image path')
            current[name] = int(value, 16) if field == 'D' else value
        else:
            raise ValueError('unexpected loaded-image field')
    if not process_seen:
        raise ValueError('loaded-image records have no process')
    finish_file()
    return images


def running_identity(pid):
    if type(pid) is not int or pid <= 0:
        raise ValueError('running PID must be a positive integer')
    if sys.platform != 'darwin':
        raise ValueError('running-image inspection currently supports macOS only')

    def start_time():
        value = subprocess.check_output(
            ['/bin/ps', '-p', str(pid), '-o', 'lstart='], text=True,
            stderr=subprocess.PIPE, timeout=5).strip()
        if not value:
            raise ValueError('process start time is unavailable')
        return value

    started = start_time()
    output = subprocess.check_output(
        ['/usr/sbin/lsof', '-a', '-p', str(pid), '-d', 'txt', '-FniD'],
        text=True, stderr=subprocess.PIPE, timeout=5)
    images = {(row['path'], row.get('device'), row.get('inode')) for row in mapped_images(output, pid)
              if 'path' in row and Path(row['path']).name == 'codex'}
    if len(images) != 1:
        raise ValueError('no unique loaded codex image')
    image, device, inode = images.pop()
    result = executable_identity(image)
    if device != result['image_device'] or inode != str(result['image_inode']):
        raise ValueError('loaded image device/inode differs from the current artifact')
    if start_time() != started:
        raise ValueError('PID identity changed during inspection')
    result.update(pid=pid, process_started_at=started,
                  identity_basis='loaded process image device/inode and packaged manifest')
    return result


def observe(package=None, selected_executable=None, running_pid=None):
    result = {'packaged_sha': None, 'selected_sha': None, 'running_sha': None,
              'observations': {}, 'errors': [],
              'limits': 'Explicit local inputs and packaged manifest identities only; '
                        'no artifact is executed, no qualification or promotion is inferred. '
                        'Replacement checks are not an atomic snapshot or a lease.'}
    for stage, value, inspect in (
            ('packaged', package, package_identity),
            ('selected', selected_executable, executable_identity),
            ('running', running_pid, running_identity)):
        if value is None:
            continue
        try:
            observation = inspect(value)
            result[stage + '_sha'] = observation['source']
            result['observations'][stage] = observation
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            result['errors'].append(stage + ': ' + str(error))
    return result
