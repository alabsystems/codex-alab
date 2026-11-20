# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe changes to the native app-server skill catalog in an isolated home."""
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import time
import unittest

workspace = Path(os.environ['ALAB_FEATURE_WORKSPACE'])
binary = Path(os.environ['ALAB_FEATURE_BINARY'])
sys.path.insert(0, str(workspace / 'alab/smoke'))
import capacity_exec as fixture
from quota_resume import Rpc


def write_skill(path, name, description):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f'---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n')


class SkillWireControls(unittest.TestCase):
    def test_root_creation_skill_edits_and_vendor_reinstall(self):
        policy = (workspace / 'codex-rs/app-server/src/skills_watcher.rs').read_text()
        intervals = re.findall(
            r'#\[cfg\(not\(test\)\)\]\s*const WATCHER_THROTTLE_INTERVAL:\s*Duration'
            r'\s*=\s*Duration::from_secs\((\d+)\);', policy)
        self.assertEqual(len(intervals), 1, 'review the production watcher batch interval')
        quiet = int(intervals[0]) + 2
        with tempfile.TemporaryDirectory(prefix='skill-') as directory:
            root = Path(directory).resolve()
            env = fixture.fresh_environment(root)
            (root / 'codex-home/config.toml').write_text(
                '[features]\ndaemon_auto_start=false\napps=false\nplugins=false\n'
                '[analytics]\nenabled=false\n[skills.bundled]\nenabled=true\n')
            skills = root / 'codex-home/skills'
            extra = root / 'work/extra-skills'
            vendor = skills / '.system'
            write_skill(vendor / 'vendor-control/SKILL.md', 'vendor-control', 'Before reinstall')

            def observe(process, output):
                rpc = Rpc(process, output)
                rpc.initialize()

                def changed(message):
                    return message.get('method') == 'skills/changed'

                def settle():
                    deadline = time.monotonic() + 4 * quiet
                    while rpc.poll(changed, quiet) is not None:
                        self.assertLess(time.monotonic(), deadline, 'watcher did not settle')

                def listing():
                    result = rpc.request('skills/list', {'cwds': [str(root / 'work')]})
                    self.assertEqual(len(result['data']), 1)
                    return {row['path']: row['description'] for row in result['data'][0]['skills']}

                def edit_and_observe(path, name, description):
                    settle()
                    write_skill(path, name, description)
                    self.assertIsNotNone(rpc.poll(changed, quiet * 2), f'no notification for {path}')
                    self.assertEqual(listing().get(str(path)), description)

                self.assertFalse(extra.exists())
                own_root = skills / '.alab-system'
                witness = own_root / 'cache-witness/SKILL.md'
                write_skill(witness, 'cache-witness', 'Cached description')
                rpc.request('skills/extraRoots/set', {'extraRoots': [str(extra), str(skills)]})
                rpc.wait(changed)
                initial = listing()
                own_skills = {p: desc for p, desc in initial.items() if Path(p).is_relative_to(own_root)}
                self.assertGreater(len(own_skills), 1, 'the installed ALab skill catalog is empty')
                self.assertEqual(own_skills.get(str(witness)), 'Cached description')
                user_skill = extra / 'discovered-skill/SKILL.md'
                self.assertNotIn(str(user_skill), initial)
                edit_and_observe(user_skill, 'discovered-skill', 'Created after registration')
                edit_and_observe(user_skill, 'discovered-skill', 'Edited after discovery')

                control = skills / 'local-control/SKILL.md'
                edit_and_observe(control, 'local-control', 'Legacy root is watched')
                settle()
                # Follow-up notifications for the watched control can clear the
                # listing obtained by edit_and_observe. Prime the witness after
                # those events have drained and before changing the ignored tree.
                self.assertEqual(listing().get(str(witness)), 'Cached description')
                # A change in the ignored ALab tree makes a silent cache clear observable.
                write_skill(witness, 'cache-witness', 'Changed on disk')
                self.assertIsNone(rpc.poll(changed, quiet), 'an ALab system write notified clients')
                self.assertEqual(listing().get(str(witness)), 'Cached description')
                shutil.rmtree(vendor)
                write_skill(vendor / 'vendor-control/SKILL.md', 'vendor-control', 'Reinstalled')
                (vendor / '.codex-system-skills.marker').write_text('replacement\n')
                self.assertIsNone(rpc.poll(changed, quiet), 'vendor reinstall notified clients')
                after = listing()
                self.assertEqual({p: desc for p, desc in after.items() if Path(p).is_relative_to(own_root)},
                                 own_skills)
                edit_and_observe(control, 'local-control', 'Still watched after vendor reinstall')
                self.assertEqual(listing().get(str(witness)), 'Changed on disk')

            code = fixture.run(binary, ['app-server', '--listen', 'stdio://'], root, env,
                               'skill-watch', interact=observe, timeout_seconds=20 * quiet)
            self.assertEqual(code, 0)
