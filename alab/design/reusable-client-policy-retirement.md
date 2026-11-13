<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Reusable clients after policy retirement

Status: typed pairing in `506701c9ae` passes 44 focused tests. Exact source and
execution evidence are recorded in
`alab/upstream/b32ada5320-typed-auth-pair-qualification.json`. The full gate on the
changed source and broader native image qualification remain pending.

The selected-account transport test in `backend-client/src/client/runtime_auth_tests.rs`
expects `RequestError::Policy(Revoked)` after the application's account policy is
invalidated and republished. On source `6d6ac5c9f7`, six other runtime-auth controls
pass, but this test returns an ordinary auth-build error. A diagnostic-only run with one temporary fixture print on
`a0ef1a9a3e` identifies the error as `runtime registration requires an actor credential`.
The original observation is recorded in
`alab/upstream/b32ada5320-backend-skills-observations.json`.

`AuthManager::auth_with_http_client_factory` returns `Option`. A cached credential
retains the account policy captured at publication. Its attempted pairing with the
new account's factory fails, and `RuntimeAuthBinding` turns the resulting `None` into
`Unavailable`, losing the policy cause.

## Preserve the source boundary

Invalidation can precede replacing an account. A cache read must therefore preserve
its published policy until a validated cache publication or reload occurs. It must
not recapture the current controller policy merely because native member identity
or the credential bytes still match. Previously returned credentials and factories
must remain revoked after a new publication.

The existing ALab adaptation in the plugin runtime-auth test explicitly reloads the
cache after account-policy publication. This is part of the current source-bound
contract. Source `abd94cfd2f` experimentally recaptured policy on a coherent cache
read and made all seven backend tests pass, but that candidate was rejected: cache
coherence alone does not establish that the new account policy belongs to the old
credential. The read-time recapture was removed. Its passing backend results are
not qualification evidence for the retained design.

The owned regression
`current_cache_read_requires_publication_after_policy_retirement` checks that a
getter cannot repair the pairing, an explicit cache publication can capture the
current restrictions, retained values stay revoked, and an unrelated manager
cannot adopt the new snapshot. This is a local manager/policy control, not proof of
native image retirement.

## Implemented adaptation

`AuthManager::alab_auth_with_http_client_factory` retains the existing
host-to-actual-manager selection and Source/cache/exact-member snapshot guards,
and returns a typed policy denial when credential/factory binding fails. The
optional upstream API delegates to this owned path and preserves its optional
interface. Missing or foreign native membership still exposes no cache. Neither
path changes an immutable snapshot or captures a newer policy on a getter.

Runtime selection and its refresh retry use the typed path. An ALab-owned
adapter carries `NetworkPolicyDenied` from the structured I/O error into
`RuntimeAuthError::Policy`; it does not parse error messages. The backend fixture
now explicitly reloads the actual manager after each policy publication. Its
stale-client refusal, typed policy assertions, registration count and destination
denial remain intact. The rejected read-time recapture is absent.

Qualification must cover stale and fresh clients, policy invalidation before
account replacement, same-source refresh, source replacement, missing/foreign
native members, and actual native readiness/retirement. The repository-skill tests
that require `sandbox-exec` also need a supported execution context; their permission
refusals are not passing scenarios and must not be bypassed by removing containment.
