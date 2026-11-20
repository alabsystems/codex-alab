<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Join original provider writes through completion publication

Decision: register the existing provider-write future with its actual Session
work ledger. A detached write was absent from that ledger, which could report
settled while the original RPC or completion publication was still pending.
Checking the provider mutex alone also misses the interval between releasing
the guard and enqueueing `Written`.

Commit `9408e23cfb0317b53ea5d991cb6b46a4d3ac5bb5` changes one marked upstream
call in `provider::Write::start`: the unchanged async body now uses the existing
all-platform `alab_spawn_original_work`. The same future owns the original
request handle, provider guard, write and completion sender. It is registered
before running and joined through completion publication. Canceling an observer
returns its join lease without canceling or resubmitting the write. No RPC,
result policy, running-chat provider or callback body is replaced.

The same ledger already participates in physical Session shutdown. Independent
review confirms that owned-local shutdown delegates to the existing retained
shutdown operation: close the actual client, join model/original work, then
close the same tenure. Request-only views share the ledger but close and drain
nothing. Joining the provider task requires enqueueing its completion, not UI
consumption, so it adds no dependency on an actor already leaving its loop.

One maintained native invocation on 9408e23c passes ten exact TUI methods:

- One new actual provider-list/Space/`Write::start` test contains success and
  transport-error scenarios. It holds the genuine typed Cloud command response,
  cancels only an observer, then requires the same cut to join after the guard
  release and exactly one original `Written` event. It checks one RPC and an
  unchanged running-chat provider. This is an owned command-transport fixture,
  not an embedded server or a Native grant.
- Two existing embedded-provider tests cover save/retry/current-chat behavior
  and invalidation after ambiguous failures.
- Seven existing Session-work tests cover cancellation, original picker/title
  requests, accepted descendants, cleanup admission, retirement and worker loss.

Original process 55241/start 25cb7f/completion d8f314 exits zero. Test, collector,
coverage, source-observation and guardian statuses are all zero, with ten passes,
no failed or ignored methods, and three complete authenticated library harnesses.
Only the TUI harness has matching methods; no empty-sibling order is inferred.
The new method's two scenarios are counted as one method.

Source-selected Trust 15698 executes this check in the maintained unverified
test lane, through the existing project admission and protected profile.
Before/after books cover 12,503 source inputs and 11 tools and are identical,
SHA-256 `346b3ab6ed1edb07907108dc88bef52ea773afb17bd0fe375bab1eb30615b1ae`.
Raw log SHA-256:
`3ae85590969b01946f813b59951ef36c6d16f37b41daf4041ab14cb133e4e6aa`.
The private saved packet contains 43 payloads and 12,410,688 bytes, including
eight selected immutable Git blobs and opaque native evidence metadata. Its
index is `bc48e77f11320f3936e89d15055105e54c43f9b5c96147a40dfa995e4fe547e2`.
Hash and inventory checks preserve original files without copying compiled
helpers or build caches; opaque native contexts are not interpreted.
Only the two ALab test files were formatted. The committed upstream delta is
one call line; reversing it restores every parent byte. `alab/delta.sh` records
the new hook as +1/-1, risk 8 and churn 4 against integrated upstream ce3031018e.

This is source quiescence before replacement, not migration of a Rust future
across processes. A complete scene cut must still handle the actual queued App
completion and every other owner. These tests establish no genuine owned-local
Native acceptance, whole-scene installation or application certificate. The
[merged gate record](merged-wide-lint-2026-11-18.md) retains its failed result;
no complete gate pass is inferred from this focused improvement.
