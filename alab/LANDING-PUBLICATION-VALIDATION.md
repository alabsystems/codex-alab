<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->

# Landing publication and resume defaults

A landing keeps its local point, carry and hold until the current host acknowledges
the publication. Submitted or queued input, a refused write, a replaced runtime or
changed authority leaves the point provisional. A cancelled evaluator retains its
publication and native-turn admission guards until the owned write settles. The
client rechecks current input and authority after acknowledgement before committing
the local landing.

Native aterm permission defaults also remain separate from explicit launch choices.
Local resumes carry the effective configuration; ordinary remote resumes and forks
do not fail the explicit-permission-override preflight merely because defaults exist.
Explicit CLI permission choices still count as explicit overrides.

## Validation

The final tested source is `0d46f89e8aa0d812855912c8078f9baa11df0263`, tree
`cfdf927c0c920baf15e96a47170b3fe568aad805`, using Trust 15698 on the explicitly
unverified lane. Eighteen selected top-level tests passed with zero ignored, plus
five isolated TUI child cases verified by their parent.

- Six publication controls cover pending and queued input, refused acknowledgement,
  an actual legacy-host acknowledgement raced by submission, cancellation custody,
  supported legacy spelling and rejudgement on a real claim-reconnect event.
- Four existing autopilot controls cover the published face during a native wake,
  held publication, unsupported leases and resumed authority.
- Eight unchanged controls cover the host client, compiled defaults and resume
  permissions. The TUI parent additionally verifies plain aterm defaults, user-file
  choices, explicit CLI choices, another terminal and inactive upstream behavior.

Autopilot tests ran from the final image. Unchanged client, core and TUI controls
used retained images from `60bb1c5de01d1a5930e8c6a5c225d6e11c3ecad8`; later changes
were confined to the new publication test file. Every image was compared and hashed,
checked for signatures and system-only loaders, and executed for its natural test
list before use. All validation actors closed and the images were retained.

Repository-policy lint passed with zero owned findings. The final autopilot lint
set aside three existing upstream atomic-method deprecations; the earlier expanded
five-package lint set aside seven upstream diagnostics. This is not a global
`-D warnings` or full-workspace test claim. No provider turn or physical app update
formed part of these checks. The closed detailed receipt has SHA-256
`e059284f2a2305c97d3558a1bbb801fbee72f45974e7505b447871826f7a6668`.

## Host boundary

The supported unbounded Land spelling begins at historically verified aterm 0.108.
Unsupported hosts retain the point with an update advisory and rejudge it on an
actual claim event. Full protection during the TUI's optimistic input begin also
requires the paired host's leased-standing-face rest check, landed in aterm
`e8ff647cf2ed3e7c492ae869d73ca4f3276162c9`. Public aterm through 0.147 lacks that
leased-Landed guard; the older unbounded protocol also retains its documented
unanswered-write limitation. See the ALab aterm client contract for protocol scope.
