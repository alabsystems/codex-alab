<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Isolated native source qualification child

`codex-cli`'s explicit `alab-native-source-proof` feature is a qualification
candidate, not a production default. It requires debug assertions: enabling it
in a release build is a compilation error. Its ALab-owned module carries both
`aterm session credential source v1` and
`aterm isolated source proof candidate v1`; the normal image carries neither
from this module. The private native lab must verify the candidate declaration,
actual executable/package, and ordinary native admission. No image override,
ambient environment switch, credential argument, or fake SDK Member is provided.
Do not install, adopt, or publish this qualification image as a supported release.

Invoke `codex alab-native-source-proof` in the lab's actual reserved source
constructor. Ordinary `life::capture_launch` installs the native host and the
CLI's existing startup admission runs before this debug dispatch. Configuration
is loaded by the normal `ConfigBuilder`; authentication uses the normal
`AuthManager`, direct readers, activity admission, network permits, owner refresh,
and derived constructors. The child issues no model/provider HTTP request.
The lab must still prevent unrelated configuration-provider traffic using its
normal isolated configuration and authenticated source admission.

Stdin and stdout use JSON lines. Each command is at most 4096 bytes and has only
`schema:1`, a `request` of 32 lowercase hexadecimal characters, and `action`.
At most 64 distinct requests are retained. Replaying an identical request returns
its original reply without repeating effects; changing the action under a used
request returns `refused` with `request_changed`. Invalid/unrecognized input exits
with a fixed sanitized error. No source path, digest, principal, token, capability,
or issuer response is copied to stdout.

Actions, in order:

- `startup`: load actual configuration and a native manager; retain its snapshot.
- `contextual`: retain a genuine root activity, acquire/use actual cached-manager
  network permission without an HTTP request, and load/use an independent direct
  reader under that same parent.
- `resample`: invoke actual delegated owner refresh. The native issuer controls
  the same-grant update and actual materialization; the child supplies no stamps.
- `derived`: construct a second actual manager under the retained activity.
- `workrebind`: close only the actual root member's own work transport via a
  debug-only negative fault, then call ordinary `AuthManager::auth()`, including
  before any native use exists. Native must perform a physical rebound check.
  An advanced source requires a distinct constructor-owned PRELOAD, actual load,
  actual cache report and Ready ACK. This action does not force provider refresh.
  The native lab never guesses which manager owns a socket. Refusal is not proof.
- `stop`: drop snapshots, derived manager, root activity, manager, and configuration,
  then await the retained actual native retirement observers. A local Drop or EOF
  does not substitute for Closed. The lab separately checks its complete census,
  including temporary managers/readers created by configuration loading.

Replies echo `schema`, `request`, and `action`, with `outcome` equal to `ok` or
`refused`. Refusal `reason` is a closed enum (`unavailable`, `timeout`, or
`request_changed`). Successful non-stop steps include `authState` with actual
watch-derived `auth`, `owner`, `source`, and `login` counters. These observations
are not native grants or resource-completion setters. Actual immutable member
operation/preload sequence correlation is verified at the native endpoint.
Each action has a 120-second detector; the external lab also owns its bounded
process guardian and must retain failures, not reinterpret timeout as closure.
EOF before a successful stop is an error.
