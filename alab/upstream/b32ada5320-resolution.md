<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Upstream b32ada5320 integration

The Agh fetch observed `b32ada5320b043bde03a10edab6b88fc126812b4` on
2026-11-10. Its complete three-commit range from `6dfaca4dad` is assessed in
`b32ada5320-from-6dfaca4dad-intake.json`; all decisions validate. The older
historical intake remains incomplete and is not replaced by this small range.

Merge `270afedaa6d6f862f952ddbf136de068e3024d6a` contains the complete range,
including SSH ProxyCommand, persisted attached-thread search and published OAuth
metadata discovery. Rust source merged automatically. Removal policy inspection
found no reintroduced removed paths. All ALab marker counts are unchanged from
candidate parent `788364ab2c`. The three previously reviewed structural losses
against origin/main remain documented in `c875667eb8-resolution.md`.

The only conflict was the experimental precomputed protocol archive. Both stable
and experimental exports were generated from the combined working tree through
the canonical ignored fixture writer, executed explicitly with native Trust
15698 in the protected profile. Both generator executions completed successfully;
they are generation operations, not behavioral qualification. The stable archive
was byte-identical. The generated experimental archive resolved the conflict,
preserving ALab protocol entries together with upstream's new fields.

Generated archive SHA-256:

- stable: `8db661350d242723c5d583c7ebfc4e31bbf09d3bf0252d70452d7ca37775b727`
- experimental: `158ae3c08355e7759017ce8be75af0cd52e2e40eff6c6d2968f28650cf141c76`

Generation log: `${HOME}/.ubd/codex-alignment-b32-schema-native.log`.
The lockfile change is exactly upstream's added `base64 0.22.1` dependency entry
for `codex-tcp-tunnel`; no other lock entries changed in this merge.

The full gate passed on exact source `270afedaa6`; see
`b32ada5320-native-gate.json`. Seventeen failed upstream library copies also
failed on pristine upstream and remain recorded as failures, not passes. Four
existing owned tests remain ignored; ALab Tippy is clean. The focused run on
`b9e8ee65e5` completed with exit 0: eight selections, 34 passed, none failed or
ignored. `b32ada5320-focused-qualification.json` records the exact source, log hash
and filters. This covers both protocol archives, OAuth metadata discovery, the
tunnel library, local search, attached-thread restart/recovery, search filtering
and the actual CLI cloud-policy fixture. Combined stdout/stderr was checked for
explicit fixture skips; a skipped subprocess does not count as security evidence.
The shared focused capture helper now captures both streams for future runs.
These focused observations do not replace the full gate. Later combined-auth
source-policy changes require their own gate; the `270afedaa6` receipt does not
qualify those changes. The older passing
full-gate receipt covers source `bccbcddbb2` and upstream `6dfaca4dad`; it does not
qualify this integration.

No package has been selected or installed for this candidate, and origin/main has
not advanced through this integration. Live Portals/OAuth eligibility, Windows
runtime checks, complete feature contracts and remaining historical review stay
separate from source ancestry and deterministic fixture results.
