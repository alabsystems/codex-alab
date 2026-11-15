<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Publish only the required graph

Publishing the model-goal restart recovery timed out after 120 seconds while
`github_push.py` asked Agh to clone a complete mirror of every remote ref. The
timeout occurred before any push or remote write. Repeating the same all-ref
download would leave a verified source change needlessly dependent on unrelated
branch and tag history.

Decision: initialize a fresh bare transport and preload the exact candidate's
complete graph from the local source. Then use the existing admitted Agh reader
to fetch authoritative main into `refs/remotes/origin/main`. Since the candidate
already contains expected main, the network transfers only missing objects.
The transport imports no unrelated branch or tag and makes no assumption about
the remote's default branch. It is never shallow.

The source repository's guards are unchanged. Agh still screens the new transport
and verifies complete ancestry, connectivity and the exact remote lease before
its atomic push. Remote rewrites, non-forward candidates, authentication failures
and uncertain write outcomes retain their original refusal paths. Forward races
remain classified only after a fresh authoritative read proves them. No timeout,
credential, permission or native source-screen exception is introduced.

Regressions require a complete candidate graph, only authoritative main, and
rejection of a candidate lost during local transfer before any remote request.
The transport and read suites exercise real local Git repositories, including
a different remote default branch, missing source objects, forward races, rewrites,
typed and untyped native failures, and unchanged source push guards.

Validation: all 26 GitHub read/publication tests pass in 16.709 seconds with the
managed Python interpreter. Independent review found no blocking issue in source
admission, cache reuse, captured candidate identity or conflict classification.
Git whitespace checks pass. The original timed-out attempt left remote main at
`afb3a00a0c81e185e823df760dff294b2526d90e`, confirmed through Agh before retrying.
The revised path then passed ordinary native Agh source admission and atomically
published `f88d0d8247bc78d33a77247bbe20cde653fc8b9d` to main. A separate Agh API
read confirmed that exact remote head; no source guard or timeout was changed.
