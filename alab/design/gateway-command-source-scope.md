<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Gateway command credentials and primary source changes

Status: source-scoped cache and request-specific recovery implemented and
focused-qualified; full native gate and broader native credential-image contracts
remain to be qualified.

The primary-policy visibility part now has a delegation hook and two real HTTP
regressions. Both fail before the hook because unavailable primary permission and
a foreign caller policy still allow dispatch. Both pass after it in the full
144-test model-provider library selection, recorded in
`alab/upstream/b32ada5320-combined-auth-source-qualification.json`. The cache/source-transition fix
now passes the unchanged gateway/primary recovery fixture. Its separate execution
records and source/isolation/cancellation controls are in
`alab/upstream/b32ada5320-gateway-source-qualification.json`; the earlier composition
refusal tests alone did not qualify this transition.

On source `d15035ac5d`, the two `command_gateway_reexports_after_401` variants
and the second-unauthorized-WebSocket test pass. The combined gateway/primary
recovery fixture fails before sending any request. A tracing-only wrapper around
the exact unchanged fixture reveals `application network permission was revoked`.
The non-gateway `external_auth_401_retry_uses_refreshed_chatgpt_headers` control
passes. The wrapper was removed after the run. Exact logs, hashes and status are in
`alab/upstream/b32ada5320-gateway-recovery-observations.json`; pristine integration
test attribution is still absent.

The gateway exporter captures `owner.application_network_policy()` when the
provider is constructed. ALab binds that policy to an immutable credential source.
Replacing startup API-key auth with external ChatGPT auth retires the captured
source. The exporter's later `for_current_account()` updates upstream's account
binding, but cannot turn that retired native source into the new source. The
ordinary request path selects its current primary snapshot successfully.

## Required behavior

1. Select the primary request source before selecting or running a gateway command.
   Retain source-use admission through actual command completion, including caller
   cancellation. A source change during preparation must refuse or restart coherent
   preparation; it must not materialize another source's credentials for old work.
2. Scope the upstream shared export cache by owner, provider configuration and exact
   selected source/policy. The same owner `Arc` alone does not identify a credential
   epoch. Catalog and inference may share only the same complete cache identity.
3. Preserve old captured exports as old-source objects that refuse use after
   retirement. Create/select a distinct export for a new source; never refresh an
   old binding by discarding native restrictions or transplanting another reader.
4. A 401 invalidates the export used by that rejected request. A late old-source
   response must not invalidate a newer source's cached export. Keep one provider
   recovery attempt and subsequent primary-auth recovery as upstream specifies.
5. Combined authentication must retain primary source-policy visibility through
   request preparation, HTTP bodies and WebSocket lifetimes. Adding a gateway header
   must not hide the primary auth provider's source contract.
6. Keep command network-policy admission, destination restrictions, sensitive
   headers, same-origin catalog requirements and redirect refusal. Preserve OAuth
   gateway behavior and catalog's prohibition on opening an interactive browser.

## Implementation boundary and qualification

Use the existing upstream export cache and command runner. Keep ALab source-key,
lease and composition adaptation in owned glue with small marked hooks. Avoid a
parallel cache, copied provider implementation, globally dynamic account binding,
or weakening `NetworkPolicy` revocation to make the fixture pass. Inspect recovery
context before choosing the cache interface: its current no-argument invalidation
does not identify which concurrent request's source was rejected.

The original gateway and non-gateway fixtures must pass without diagnostics or
changed expectations. Add coverage for a source replacement followed by gateway
401, shared catalog/inference cache identity, unrelated source separation, late
old-source rejection after a new-source cache is populated, and cancellation while
an actual command is running. Use native source-lifetime fixtures for native claims;
an ordinary fake token or a policy-field equality test does not establish those
contracts. Qualify changed code with the native gate after focused checks. This
adaptation is unfinished until those observations support it.

## Implemented adaptation

Owned glue selects an upstream export using owner, provider configuration and the
exact primary/caller policy conjunction. The existing upstream cache retains the
only token store. A selector retains the current export so catalog and inference
can share the same complete key; a captured export never recaptures a new account.
A generic owned credential-operation wrapper retains source admission and actual
command ownership through completion even when its caller is cancelled.

Combined auth carries a recovery context containing the actual export and token
generation. Core HTTP and WebSocket recovery retain that request auth and invoke
its optional recovery future within upstream's existing retry budget. Non-gateway
providers retain their original recovery behavior. Cache invalidation compares the
rejected token generation, including when a later export has identical bytes; it
does not look up the manager's current cache. Native credential-image lifecycle
claims still require their own contract evidence beyond the local controls.
