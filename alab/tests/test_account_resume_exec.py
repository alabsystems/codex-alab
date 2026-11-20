# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "smoke"))
import account_resume


class ExecResumeEvidence(unittest.TestCase):
    def test_exec_seed_requires_the_real_origin_id_and_cwd_in_the_saved_thread(self):
        thread = "00000000-0000-4000-8000-000000000009"
        turn = "00000000-0000-4000-8000-000000000008"
        binary = Path("/fixture/codex")
        prompt, answer = account_resume.terminal.SEED, account_resume.terminal.REPLY
        args = ["exec", "--json", prompt]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "seed-guardian-result.json").write_text(json.dumps({
                "argv": [str(binary), *args], "returncode": 0, "timed_out": False,
            }))
            events = [{"type": "thread.started", "thread_id": thread}, {"type": "turn.started"},
                      {"type": "item.completed", "item": {"type": "agent_message", "text": answer}},
                      {"type": "turn.completed"}]
            (root / "seed-stdout.jsonl").write_text("".join(json.dumps(row) + "\n" for row in events))
            sessions = root / "codex-home/sessions/2026/11/19"
            sessions.mkdir(parents=True)
            rollout = sessions / f"rollout-seed-{thread}.jsonl"
            meta = {"id": thread, "source": "exec", "cwd": str(root / "work")}
            done = {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": turn,
                                                    "last_agent_message": answer}}
            with patch.object(account_resume.fixture, "run", return_value=0):
                for change in ({}, {"source": "cli"}, {"id": "another-thread"}, {"cwd": str(root)}):
                    with self.subTest(change=change):
                        rollout.write_text(json.dumps({"type": "session_meta", "payload": meta | change})
                                           + "\n" + json.dumps(done) + "\n")
                        if change:
                            with self.assertRaisesRegex(RuntimeError, "original rollout"):
                                account_resume.exec_turn(binary, args, root, {}, "seed", answer)
                        else:
                            self.assertEqual(account_resume.exec_turn(binary, args, root, {}, "seed", answer),
                                             (thread, turn))

    def test_guarded_command_launches_in_the_requested_sibling_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            work = root / "work"
            sibling = root / "second-project"
            work.mkdir()
            sibling.mkdir()
            env = {"PATH": "/usr/bin:/bin"}
            for label, cwd, expected in (("default", None, work), ("sibling", sibling, sibling)):
                status = account_resume.fixture.run(Path("/bin/pwd"), ["-P"], root, env, label, cwd=cwd)
                self.assertEqual(status, 0)
                self.assertEqual((root / f"{label}-stdout.jsonl").read_text().strip(), str(expected.resolve()))
            with self.assertRaisesRegex(RuntimeError, "outside the original workspace"):
                account_resume.exec_resume(Path("/unused"), root, env, "unused", launch_cwd=work)

    def test_reply_must_belong_to_the_original_completed_turn(self):
        thread = "00000000-0000-4000-8000-000000000001"
        turn = "00000000-0000-4000-8000-000000000002"
        binary = Path("/fixture/codex")
        answer = account_resume.terminal.ANSWER
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            argv = [str(binary), "exec", "--json", "--skip-git-repo-check", "resume",
                    thread, account_resume.terminal.CONTINUE]
            (root / "continued-guardian-result.json").write_text(json.dumps({
                "argv": argv, "returncode": 0, "timed_out": False,
            }))
            sessions = root / "codex-home/sessions/2026/11/19"
            sessions.mkdir(parents=True)
            rollout = sessions / f"rollout-date-{thread}.jsonl"
            rollout.write_text(json.dumps({"type": "event_msg", "payload": {
                "type": "task_complete", "turn_id": turn, "last_agent_message": answer,
            }}) + "\n")
            started = {"type": "thread.started", "thread_id": thread}
            begin = {"type": "turn.started"}
            reply = {"type": "item.completed", "item": {"type": "agent_message", "text": answer}}
            end = {"type": "turn.completed"}
            stdout = root / "continued-stdout.jsonl"

            def emit(rows):
                stdout.write_text("".join(json.dumps(row) + "\n" for row in rows))

            with patch.object(account_resume.fixture, "run", return_value=0) as run:
                emit([started, begin, reply, end])
                self.assertEqual(account_resume.exec_resume(binary, root, {}, thread), turn)
                for rows in ([started, begin, end, reply],
                             [dict(started, thread_id="different-thread"), begin, reply, end],
                             [started, begin, reply, {"type": "turn.failed"}, end]):
                    with self.subTest(rows=rows):
                        emit(rows)
                        with self.assertRaisesRegex(RuntimeError, "wrong thread, reply or turn boundaries"):
                            account_resume.exec_resume(binary, root, {}, thread)
                work = root / "work"
                sibling = root / "second-project"
                work.mkdir()
                sibling.mkdir()
                emit([started, begin, reply, end])
                with self.assertRaisesRegex(RuntimeError, "requested thread"):
                    account_resume.exec_resume(binary, root, {}, thread, launch_cwd=sibling)
                target_argv = [*argv[:3], "-C", str(work), *argv[4:]]
                (root / "continued-guardian-result.json").write_text(json.dumps({
                    "argv": target_argv, "returncode": 0, "timed_out": False,
                }))
                self.assertEqual(account_resume.exec_resume(binary, root, {}, thread, launch_cwd=sibling), turn)
                run.assert_called_with(binary, target_argv[1:], root, {}, "continued", cwd=sibling)
