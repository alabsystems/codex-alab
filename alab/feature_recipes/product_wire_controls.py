# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Exercise packaged policy and user overrides through the public CLI."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

workspace = Path(os.environ['ALAB_FEATURE_WORKSPACE'])
binary = Path(os.environ['ALAB_FEATURE_BINARY'])
sys.path.insert(0, str(workspace / 'alab/smoke'))
import capacity_exec as fixture
import model_resume
from quota_resume import Rpc


class ProductWireControls(unittest.TestCase):
    def test_terminal_defaults_and_runtime_override_precedence(self):
        for in_aterm in (False, True):
            with self.subTest(in_aterm=in_aterm), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                env = fixture.fresh_environment(root)
                if in_aterm:
                    env['TERM_PROGRAM'] = 'aterm'
                home = root / 'codex-home'
                (home / 'config.toml').write_text('features.daemon_auto_start=false\nanalytics.enabled=false\n')

                def defaults(process, log):
                    rpc = Rpc(process, log)
                    rpc.initialize()
                    config = rpc.request('config/read', {'cwd': str(root / 'work')})['config']
                    self.assertEqual(config['model_reasoning_effort'], 'ultra')
                    self.assertEqual(config['tui']['notification_method'], 'osc9' if in_aterm else 'auto')

                self.assertEqual(fixture.run(binary, ['app-server', '--listen', 'stdio://'],
                                             root, env, 'defaults', interact=defaults), 0)
                self.check_runtime_layers(root, env)

    def check_runtime_layers(self, root, env):
        home = root / 'codex-home'
        work = root / 'work'
        events = []
        gateway = model_resume.Gateway(events)
        server, service = fixture.start_listener(gateway.handler())
        try:
            model_resume.configure(root, server.server_port, 'low')
            (home / 'auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'sk-loopback-fixture'}))
            (home / 'editing.config.toml').write_text('model_reasoning_effort="high"\n')
            # The profile overrides the user file. A trusted project and explicit command
            # overrides then take precedence over both. Inspect each actual Responses POST.
            for layer, expected in [('user', 'low'), ('profile', 'high'), ('project', 'xhigh'), ('cli', 'low')]:
                args = ['exec', '--skip-git-repo-check', '--json']
                if layer != 'user':
                    args.extend(['--profile', 'editing'])
                if layer == 'project':
                    (work / '.codex').mkdir()
                    (work / '.codex/config.toml').write_text('model_reasoning_effort="xhigh"\n')
                if layer == 'cli':
                    args.extend(['-c', 'model_reasoning_effort="low"'])
                args.append('Reply with one word. Do not use tools.')
                before = len(events)
                with self.subTest(layer=layer):
                    self.assertEqual(fixture.run(binary, args, root, env, layer), 0)
                    posts = [(r['model'], r['effort']) for r in events[before:] if r['event'] == 'request']
                    self.assertEqual(posts, [(model_resume.OLD, expected)])
        finally:
            server.shutdown()
            server.server_close()
            service.join()
