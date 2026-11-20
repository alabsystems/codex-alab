<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Integrating the Tab 3 command execution branches

## Required PTY working directory

The qualification branch at `57bd0667d2` recorded that a command could run in
the home directory after its requested working directory disappeared. The
event still named the requested directory. The Unix PTY launcher now passes
an explicitly supplied directory through to child creation. The operating
system can reject it before executing the command. Omitting the directory
still selects the ordinary default. The Windows adapter supplies the explicit
directory to CreateProcessW and rejects empty paths and embedded NULs.

The vendored crate matches the archive digest pinned by the upstream lock.
The sole source difference in its recorded inputs is `src/cmdbuilder.rs`.
The pristine comparison continues to use the upstream crate. All 20
certified-build dispatch tests passed. On the combined source
`a8172ed196`, the locked macOS run passed all 59 PTY tests and all four
app-server command-notification cases, including the missing directory.
Running the missing-directory regression with the original registry backend
reproduced the fallback. The Windows library and its tests cross-compiled
with stock Rust 1.97.1 because the pinned Trust installation only provides
the Darwin standard library. Windows execution remains unverified.

## Command results and retained history

The branch at `74e0f041bc` includes the earlier `8bb55a6b3c` command-result
branch. It keeps the observed process exit and measured duration separate
from completion status across execution, the public event, history and the
display. A launch refusal can have neither measurement. A process that
actually exited can have a real zero or negative exit code.

The candidate also began writing optional exit codes and reported terminal
status inside retained TUI cells while still labeling the graph as version 5.
The previous reader required an integer exit code and rejected unknown fields.
Reading the new normal-completion record with that reader reproduced the
incompatibility. `66efe1af2c` versions the inner graph as 6 and reads both
formats. During a version 5 restore, a still-streaming preview loses its
placeholder zero and a finalized command retains its numeric result. The
outer runtime format and receiver activation use their existing policy.

The pinned Trust check rebuilt the protocol and trace dependency chain.
All 74 rollout-trace tests passed. Independent wire checks covered all nine
combinations of missing, false and true observation markers, six malformed
values and 36 completion round trips. The legacy TypeScript declaration for
duration uses a string while Serde encodes seconds and nanoseconds. This
mismatch also exists in the parent of the command-outcomes branch.

On the composed source `b42945fb5c`, four public app-server cases passed,
including the real process start and all three launch failures. Across five
Rust libraries, 22 focused tests passed. The broader reruns passed 44 TUI
tests covering history and rendering, 32 Core execution tests, all 77 exec
library tests and all 74 rollout-trace tests. Both app-server runs exercised
the mock server and real command-notification path on macOS.

The full protocol run exposed four generated-fixture failures. The outcome
change added two optional observations and revised the exit description in
the stable internal rollout schema. The repository already declared the
retained-configured notification in its public protocol, and its existing
history types included input review and retained-side metadata.
`79ffdbeff1` regenerates the public JSON and TypeScript fixtures, the stable
and experimental compressed exports, and the derived Python SDK from the
composed source. All 1,130 fixture files matched a fresh native generation
byte for byte, and all ten Python SDK contract tests passed.

All three original qualification tips are ancestors of the published main.
The qualified PTY merge is `71d0453a14`, and the command outcomes and
generated artifacts landed in `c54000fe47`. The six local topic branches
were already merged and have attached worktrees. Published release,
alignment and qualification refs identify source used for review. The
retained-session receiver still has a separate activation hold.

After regeneration the existing native test executable passed 333 protocol
tests, including both public fixture comparisons. One writer test is ignored
in ordinary runs and was invoked to generate the fixtures. Two archive
assertions embed data at link time. The old binary still contained the old
archives, so their new contents were checked against the native writers
separately. The protected full gate stopped at its 20 GiB free-space floor
during lint. Two attempts to rebuild the protocol tests with standalone
Cargo feature selection also reached the floor. The preserved build
artifacts remain available for a repeat run.

The saved Cargo fingerprint resolves all 29 dependencies of the earlier
native protocol test executable. Of its 85 source inputs, only the two
embedded archives changed during regeneration. A guarded single-crate
relink was prepared in the detached schema checkout. Its final space wait
expired with 10.54 GiB free, and the compiler did not start. On this Mac
the exact retry is `/bin/bash /private/tmp/outcomes-schema-qa.a35dfejN/run-relink.sh`.
The portable repeat is the source-selected, locked
`targo --unverified test --locked -p codex-app-server-protocol --lib`
from `codex-rs/`, followed by the repository gate.
