# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""A Linux bind can cover an ordinary file or a directory while its st_dev stays the same."""

from io import BytesIO
from pathlib import Path
import sys
import unittest
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from sync_worktree import check_cleanup_target, check_linux_mounts


def mount(where, root=b"/"):
    return b"60 40 8:2 " + root + b" " + where + b" rw shared:1 x:future - ext4 /dev/root rw\n"


class CleanupMountTests(unittest.TestCase):
    def test_same_device_file_and_directory_binds_and_encoded_names(self):
        workspace = b"/cache space/tab\tnewline\nback\\slash/"
        encoded = rb"/cache\040space/tab\011newline\012back\134slash/"
        self.assertIsNone(check_linux_mounts(workspace + b"codex-rs", workspace + b"codex-rs/Target", BytesIO(
            mount(b"/") + mount(encoded + b"codex-rs/target-backup", b"/outside"))))
        for suffix in (b"codex-rs", b"codex-rs/Target", b"codex-rs/Target/debug",
                       b"codex-rs/Target/debug/held-proof.json"):
            with self.subTest(suffix=suffix), self.assertRaisesRegex(ValueError, "mounted path"):
                check_linux_mounts(workspace + b"codex-rs", workspace + b"codex-rs/Target",
                                   BytesIO(mount(b"/") + mount(encoded + suffix, b"/outside")))

    def test_unknown_or_incomplete_mount_inventory_blocks_cleanup(self):
        for data in (b"", b"incomplete\n", mount(b"/")[:-1], mount(rb"/unsafe\02name"),
                     mount(b"relative"), mount(b"/") + b"partial"):
            with self.subTest(data=data), self.assertRaises(ValueError):
                check_linux_mounts(b"/repo/codex-rs", b"/repo/codex-rs/target", BytesIO(data))

    def test_custody_refuses_an_unreadable_process_mount_table(self):
        with patch("sync_worktree.sys.platform", "linux"), patch("sync_worktree.os.fstat"), \
                patch("sync_worktree.os.readlink", side_effect=[b"/repo/codex-rs", b"/repo/codex-rs/target"]), \
                patch("builtins.open", side_effect=PermissionError("proc unavailable")) as opening:
            with self.assertRaisesRegex(PermissionError, "proc unavailable"):
                check_cleanup_target(Path("/repo/.git"), (1, 2, 3))
            opening.assert_called_once_with("/proc/self/mountinfo", "rb")

    @unittest.skipUnless(sys.platform.startswith("linux"), "requires the Linux process mount namespace")
    def test_reads_the_kernel_table(self):
        with open("/proc/self/mountinfo", "rb") as mounts:
            check_linux_mounts(b"/nonexistent-codex-test/workspace", b"/nonexistent-codex-test/workspace/target", mounts)
