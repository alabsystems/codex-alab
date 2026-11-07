<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Destination configuration ownership

Status: source audit and proposed interfaces, 2026-11-07. No implementation,
compilation, runtime qualification, provider acceptance, or SourceV1 activation
is claimed. Reviewed the transport worktree at `f4d1efab9c` and the live
`codex-accounts-view` destination-epoch draft read-only. The latter has uncommitted
source, so its method names below describe that draft, not a landing receipt.

A is the retiring account; B is the installed destination; C is the live local
and administrator configuration authority. B needs a separate account policy
owner while continuing to obey C. New local owner identity is not native grant,
policy, resource-readiness, or publication authority.

## Existing seams and traps

- `alab-core/glue/auth_native_route.rs` implements
  `AuthRouteConfig::alab_fresh_destination_routes() -> io::Result<(Self,
  NetworkPolicyController)>`. It creates an unavailable B controller, preserves
  endpoint limits and the local bootstrap factory, removes cookies, and rejects
  credential/native-bound configuration. The returned controller must be the
  actual controller later owned by B's configuration manager.
- `app-server/src/config_manager.rs::ConfigManager` is `pub(crate)`. Its derived
  Clone shares the cloud loader cell, effective network controller, composed
  snapshot, reload semaphore, and EMA cancellation roots. Clone alone is not a B
  fork. `new` creates fresh ownership, but snapshots CLI overrides and starts a
  new empty runtime-feature map; reconstructing from copied Config alone loses
  the live local owner.
- `application_network.rs::refresh_application_network_policy` reads live local
  application requirements, fetches its cloud snapshot, composes system/cloud/MDM
  in their real order, and publishes through its controller. Its load proof is
  checked by exact revision and Arc identity. B must keep this mechanism.
- `load_startup_config` may fall back to `load_default_config`; the fallback can
  ignore managed requirements after local-policy failure. It is not the B
  admission path. Even a successful initial `load_latest_config` with an empty
  cloud loader publishes local-only application policy. That must not make B
  generally usable before B's account requirements have been fetched.
- `update_managed_config_loader` keeps an existing config-server loader and
  refresh worker, returning early. A copied ChatGPT loader or cloud snapshot must
  never be passed off as B's requirements. `clear_chatgpt_config_loader` likewise
  neither revokes the effective network snapshot nor guarantees a new owner.
- `in_process_bootstrap.rs::configure` performs several AuthManager constructions
  and uses startup fallback. It cannot be reused unchanged for B. Every new
  bootstrap/policy/serving manager must be registered under the actual destination
  constructor/census; ordinary constructors must not fall back to launch A.

## Proposed owner interface

Implement the policy-independent orchestration in ALab-owned glue, with only a
stable include/call hook in app-server. The following names are proposals, not
existing callable APIs:

```
ConfigManager::alab_begin_destination(routes, controller)
    -> io::Result<DestinationConfigOwner>
DestinationConfigOwner::local_bootstrap(cwd)
    -> io::Result<DestinationBootstrap>
DestinationConfigOwner::load_for(resources, bootstrap)
    -> io::Result<LoadedDestinationConfig>
LoadedDestinationConfig::check_current(resources)
    -> io::Result<()>
```

`routes` and `controller` are the exact pair produced by the fresh-route factory.
The owner is not a serde value or a publicly constructible bag of Config fields.
Its final publication authority comes from the resource owner, not this factory.

Build the B ConfigManager explicitly, without cloning A's account ownership:

| Input/state | B treatment |
| --- | --- |
| CLI overrides and runtime feature enablement | Share the actual live C owners; do not freeze a copied Vec/map. |
| Local network controller | Retain the actual live C controller, including later revocation. |
| Loader overrides, strictness, Codex home, argv0, project context | Preserve the actual caller's configuration semantics. |
| Thread config loader | Share only if its ownership is account-independent C; otherwise rebuild a B-bound loader or refuse. A's remote/account loader is not implicitly C. |
| Effective network controller | Use the exact fresh B controller, initially unavailable. |
| Cloud loader cell | Fresh cell; construct B's account loader from actual B auth. |
| Composed snapshot | Fresh `None`; never copy A's ApplicationPolicySnapshot. |
| Reload semaphore | Fresh owner, coordinated with the live C revision. |
| EMA startup/lifetime roots | Fresh roots; B cannot inherit A's cancellation/ready state. |

Do not retire a shared administrator/config-server owner by mistaking it for an
A account loader. Preserve the actual current config-server requirements and
transport constraints from C. Either retain its independently established live
administrator owner, with explicit provenance, or create the normal config-server
loader from freshly loaded C requirements. Never select this branch using a
label, endpoint guess, or an A cloud snapshot. The existing
`managed_config_bundle_loader` already chooses config-server versus ChatGPT from
the actual typed requirements; use that selection with the correct owner.

## Required ordering

1. Capture the original manager's pre-effect target policy and actual live C
   inputs. Obtain the fresh routes/controller pair. B effective policy remains
   unavailable. Source retirement invalidates A's composed policy/snapshot and
   EMA/startup admissions using actual retirement authority; it does not revoke
   the shared local C controller or independent global administrator authority.
2. `local_bootstrap` refreshes local requirements using
   `load_local_application_requirements` and the actual loader overrides. Load
   the required local/global auth configuration without an A cloud layer. This
   load must not call `refresh_application_network_policy`, publish local-only
   policy into B, or use a default/ignore-requirements fallback. Fail closed on
   unavailable/invalid local or administrator policy.
3. Construct the real B manager through the draft
   `AuthManager::alab_prepare_destination(prepared, destination, seal, config)`.
   Preserve every original actor's actual constraints and validate B's document.
   All credential reads and requirements HTTP occur within the exact
   `DestinationEpoch::with_resources` / `DestinationResourceContext::with_resources`
   scope, including any independently created policy/bootstrap managers. No
   supplied path, task ID, local controller, or Config is native authority.
4. Build B's managed requirements loader with actual B auth and C's constrained
   bootstrap transport. Generic B traffic stays unavailable; bootstrap permits
   only the required endpoints under live C. Do not load requirements using A's
   manager, cookies, cached bundle, or source-bound factory. No detached refresh
   task may escape its real B source/use lifetime.
5. Fetch B requirements and compose them with current system and MDM constraints
   using the existing normal composition/load-proof mechanism. The final load is
   fail-closed `load_latest_config`/equivalent, never `load_startup_config`.
   Verify the original C inputs/revision, the B controller, exact load proof,
   destination membership and actual credential scope before and after loading.
   Validate B requirements against every original actor constraint; a new B
   constraint cannot be ignored because a manager was constructed earlier.
6. Initialize all B resources from this B Config and manager: policy loaders,
   MCP/Apps/EMA, model/HTTP/WS clients, analytics and any independent managers.
   Keep the config owner, B loader/refresher and controller with those resources.
   A loaded Config or one Ready member is not a complete resource bundle.
7. Only the real resource owner may publish the full B bundle together with the
   matching `DestinationAppliedProof` and current resource binding. Reconcile
   legitimate post-ACK C changes without changing the historical ACK report.
   Atomically switch manager, Config/config-owner and resources together; never
   expose B credentials with A's ConfigManager or A clients with B policy. Any
   failed preparation leaves B unavailable and retains truthful retirement state.

## Call-site work still required

- Add the explicit fork/local-bootstrap hook beside ConfigManager and
  `application_network.rs`; no current constructor accepts the fresh returned B
  controller while preserving all live C owners.
- Adapt the owner of `InProcessStartArgs` / `start_uninitialized`, rather than
  recursively invoking the current startup bootstrap. `EmbeddedNetworkPolicy`
  fields are crate-private; it currently has no public typed B-owner constructor.
- Connect the parent destination-resource orchestration to the draft
  `alab_prepare_destination`, resource context and Applied proof. Existing
  `assignment_lifecycle.rs::SessionAction::Destination` and `prepare_session`
  still operate on the old session resource path; their success is not evidence
  that a separate B ConfigManager/full bundle was built or atomically published.
- Add a narrow revocation operation that clears A's composed snapshot, invalidates
  only its effective controller and retires its EMA/account loader as appropriate.
  `revoke_mcp_ema` alone does not invalidate the network policy/snapshot.
- Establish a typed local-input revision/join if the current mutable CLI/runtime
  owners cannot be checked coherently across the asynchronous B load. Sharing
  Arcs prevents a stale copy but does not itself prove a coherent load.

Required controls include changed C during B fetch, unavailable B requirements,
config-server/global-admin constraints, A-only allowed versus B-denied endpoints,
no local-only generic B contact, fresh B cookies/loader/EMA ownership, fallback
refusal, B refresh during resource initialization, failed bundle construction,
and atomic publication requiring the actual full bundle and native Applied
proof. These remain unimplemented/unrun in this document-only slice.
