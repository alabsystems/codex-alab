# Package controller root cancellation

The native controller intentionally forwards signals only to its owned unreaped
root and retains borrowed resources until its native cohort drains. It does not
turn a late signal into permission to kill arbitrary descendants. The package
selected entry left a Bash root waiting for a Python guardian, whose custodian
owns a separate writer group. Signalling that shell root could leave the guardian
and cache lock live. This establishes a cancellation boundary defect; it does not
establish the original build wait's cause or validate the monitor's stuck verdict.

A controlled local comparison reproduced root exit -15 with the lock still held
for the shell wrapper, versus guardian exit 143 with the lock released for direct
Python execution. The selected-entry regression also failed before the change:
root exit preceded writer retirement and lock release. All inert writers were
released through fixture sentinels; no numeric descendant PID was signalled.

Decision: the executable package selected entry now becomes the Python guardian
with exec, preserving the root PID the native front owns. The returning library
helper remains available. Both share command preparation. Post-package native
maintenance now runs in the guardian CLI after the producer lock is released,
including its failure path; ordinary cache clients remain unchanged. Maintenance
uncertainty does not replace the build status. A refused lock symlink cannot
redirect maintenance to its target's cache: resolve the named parent, not the leaf.
No controller signal authority or source-custody acceptance rule was broadened.

Local verification under managed Python/Git:

- Package-cache orchestration: 12 passed in 49.576s, including INT/TERM/HUP sent
  only to the selected-entry root, and successful/failed executable-root cases
  retaining maintenance order and exact status.
- Guardian: 22 passed in 1.332s, including a new refused-lock-symlink control.
- Post-fork guardian: one passed in 0.100s.
- Package source custody: 26 passed, one existing opt-in native-cache test skipped,
  across 27 methods in 88.161s.
- Bash syntax and diff whitespace checks passed.

Real native qualification ran in an isolated m157 checkout at exact
`e56cf5fb99ec3ceb4fa9d06a8b850324f8beaf1d`, leaving the older staging checkout
and producer fixed. The frozen qualified atpkg-publish from aterm 08d1cd2d3b
performed actual working-lock selection and supervised inert writers; no compiler
ran and no authentication fields were fabricated. Both native test methods passed
without skips in 44.117s (owned shell s-25205fe33e907f1d5dc8, block 9, complete
exit 0). Three positive cases cover INT/TERM/HUP. The returning-shell negative
control proves native custody retains the live guardian until explicit writer
release. Every test waits for its native front and verifies lock release.

Complete native log:
/tmp/.ubd/codex-native-f0a4e96f.8bYgIz/package-controller-native-cancellation.log,
665 bytes, SHA-256 c6867f0a37475d0c08d7916d1a22b5009ce1623417d58bebad0893fc3338e6ea.
Block 10 confirmed this checksum and the clean exact qualification checkout. It
also found the older staging cache lock available: that attempt's supported
custody had finally released. No receipt was forced Ready and no broad remote
process inspection or bare-PID cleanup was used. An actual staged artifact and
installed adoption still require their own completed checks.

The subsequent standard `package.sh --commit fb725e80a3 --no-promote` run
completed successfully (same owned shell, block 12, exit 0), using the exact
qualified packager e56cf5fb99 and frozen native producer. Its final nonhidden
artifact records source fb725e80a3d1ece0cc19792de0af804de7a7d4f8, packager
e56cf5fb99ec3ceb4fa9d06a8b850324f8beaf1d, admitted compiler 010b4e686f3b1ea7,
and build time 2026-11-07T06:20:11Z. Package version is
0.1.0-dev.15016+codex.0.167.0.gfb725e80a; protocol client version is
0.168.0-dev.15016+gfb725e80a.

All standard contained smoke checks passed: three executable versions, protocol
client identity, compiled defaults, upstream identity, bundled PCRE2 ripgrep,
strict code-signature verification, arm64 architecture, system-only libraries,
and layout validation. The final 409 MB artifact is retained on m157 at
~/.local/share/aterm/dev-builds/opening-s0-fb725e80a3/codex/0.1.0-dev.15016+codex.0.167.0.gfb725e80a.
Block 13 independently read the final metadata and found all four source guard
receipts closed. The native foreground completed normally; no process cleanup
or custody repair was needed.

Complete successful log: same log directory, file
package-opening-fb725e80a3-cooperative-root.log, 49859 bytes, SHA-256
9ddff01be2b0ba68e1323c10db44f2ba47a48f53610759d430c28b9c643ff0f0.
Decision: retain this private, smoke-qualified artifact as deployment evidence.
No package selection, signed release publication, installed adoption, agent
restart, or SourceV1 activation was performed. Those are distinct operations;
the completed staging result does not claim them.
