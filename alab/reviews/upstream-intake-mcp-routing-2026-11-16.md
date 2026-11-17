<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# MCP page scope, permission profiles and explicit routing

A successful maintained `alab/github.py` fetch with managed Python 3.14 observes
upstream main at `75fc67e8f4284a669eebd90ab6bc9ef7697e876e`, one commit beyond
the previous fetched `98e1584a082234ae716a3cc69d4c6f2d3631cf2d`. There are 180
pending commits beyond the integrated `ce3031018ef130e653ed4d9c5bb971652de97161`
frontier. Extend the reviewed prefix from 76 to 82 through
`a46c5be77eac0dc856f677a099a6a7702edd578b`, leaving 98 beyond this review.

The isolated worktree began clean at review commit
`b2ec56be9e17ed9ae7ffd0ed43add9049558ed8b`. Its prior 76-commit report and
artifact match published main `d2892c7774badc1ad0de7555d569a89c61d8fe39`.
Keep every prior decision object unchanged, with historical records pointing
directly to the new active replacement. This is source review, not integration,
feature activation, native qualification, an efficiency measurement or installed
adoption. Concurrent qualification of the separate 61-commit candidate supplies
no evidence for this additional range.

## Precise Cloud status requirement for the next integration

This clarifies the existing `adapt` decision for `f8580db472` without changing
that decision. In `chatwidget/status_controls.rs::add_status_output`, upstream
now resolves the model and effort from the accepted Cloud selection. ALab's
existing status-card hook replaces upstream's model argument with
`alab_model_with_service_tier()`. That helper still takes the serving
`model_display_name()`, whereas the tier helper now looks up eligibility for
`selected_model()`. Reusing it unchanged can display one model's name and
another model's supported speed.

Add a status-specific ALab formatter that uses upstream
`status_model_display_name()` and the status model's tier eligibility, preserving
ALab Fast/Ultrafast/custom labels and their independent gates. Use it only for
the status card. Native status-line and title paths should keep upstream's
status-model/effort selection. In the optional interface, use the same status
model name when removing the literal `default` effort suffix; a serving-model
prefix cannot match a different accepted model's label.

Do not globally repoint `alab_model_with_service_tier()` at the accepted Cloud
model. `app/history_ui.rs::clear_ui_header_cell` pairs that helper with
`current_reasoning_effort()`. Its model name and tier eligibility must therefore
both come from the current serving/collaboration model and current tier, without
consulting the future accepted selection. The saved startup header's
`alab_session_model_with_service_tier(model, tier)` must continue to use its
explicit saved model/tier and saved reasoning effort, including when the active
collaboration model differs.

Require these actual-widget/request-path regressions before integration:

- Extend `cloud_model_selection_waits_for_native_admission` with ALab enabled
  and the optional interface both on and off. Exercise Started and Steered
  acceptance, definite local refusal and unknown mutation outcome. Before
  admission, the model surfaces remain serving. After acceptance, `/status`,
  configured native status line/title and optional status use the accepted
  display name/effort. Refusal and unknown outcome must not create an accepted
  selection; retain the existing held-input/draft assertions. Session metadata
  clearing the selection must return the surfaces to that session's metadata.
- Add `alab_cloud_status_selection_does_not_relabel_session_headers`: use
  distinct catalog display names, serving model A with low effort and supported
  priority/Fast, and accepted model B with high effort and no priority support.
  Render the real status card and cleared header, and read native title/status
  and optional-interface output. Status must show B/high without inventing Fast;
  the cleared header must retain A/low/Fast. Render the saved startup header too,
  retaining its explicit saved pairing even with another collaboration model.
- Add `alab_cloud_default_effort_trimming_uses_status_model_name`: accepted B
  has no explicit effort, its preset default differs from the serving config's
  effort, and its display name differs from A. The status card resolves B's
  default without falling back to A's configured effort. Native default-effort
  presentation retains upstream's contract; the optional interface suppresses
  its literal `default` word using B's name. None of these rendering operations
  changes `current_model()`, `current_reasoning_effort()`, current tier, active
  permissions, account identity or native usage/recovery ownership.
- Retain `supported_speed_tiers_reach_model_status_title_and_header_surfaces`,
  `labels_follow_independent_speed_gates_and_do_not_invent_support` and
  `startup_header_keeps_saved_model_and_reasoning_when_plan_model_differs`.
  Include disabled Fast with enabled Ultrafast, unsupported selected-model
  tiers, explicit standard/unknown tier IDs and ordinary local selection.

These are requested assertions, not claims that those scenarios currently pass.
Keep the implementation in ALab-owned glue plus the minimal status hook.

## Page-scoped MCP discovery: adopt `6d99161bce`

The general list path chooses, sorts and deduplicates complete `McpServerKey`
identities before discovery. It includes published runtime keys when choosing
the page, keeps same-named registrations in different environments distinct,
and limits both ordinary temporary discovery and EMA replacement reads to page
members. An empty general page returns the default empty snapshot without
starting its temporary connection set. Runtime statuses are refreshed after
discovery when a nonempty page may have completed an existing startup.

The shared collection API now receives the already-selected effective-server
map instead of an optional server key. No ALab caller of that API was found.
Preserve the ALab accounts hook at the auth-manager argument: pass the selected
`mcp_manager.auth_manager()` rather than the unrelated processor harness manager
that upstream still supplies. Paging is not permission to inspect another
credential owner or revive retired-source work.

Scope the efficiency claim carefully. The explicitly named thread-server branch
still obtains `mcp_server_status_snapshot` before calling `status_page`, and that
snapshot can await selected startup and read authentication. Thus this change
does not establish that every empty/invalid selected-server page performs no
discovery. It avoids unrelated/general-page work; no runtime measurement was
made here. A stricter selected-page optimization would need a separate change
and ordering test.

Require `mcp_server_status_list_discovers_only_the_requested_page` with actual
initialization counts on successive and empty general pages,
`mcp_server_status_keeps_same_named_environment_inventory_separate` with page
identity/auth/tools comparisons, connection reuse, runtime-only/disconnected
rows, failed tool discovery and tools/auth-only inventory omission. Add cursor
boundary/overflow/zero-limit and explicit-server selector coverage. Retain ALab
`status_uses_selected_mcp_owner_when_harness_storage_is_unavailable` in both
thread and threadless modes, including paginated inspection and source retirement.

## Native permission diagnostics and safe projection: adopt `400a1e1e82`

Sampling feedback and thread-start trace metadata derive historical labels
directly from native permission profiles rather than materializing a legacy
sandbox policy. The feedback tag remains `sandbox_policy`; rollout thread-start
metadata changes its field to `permissions_profile` and uses lowercase stable
labels. These are diagnostic classifications, not executor authorization.
Feedback deliberately uses the initial turn cwd. Thread metadata uses the
materialized profile for its selected environments.

The extracted `with_workspace_write_cwd` retains default read-only metadata
subpaths and the existing explicit-rule precedence. The TUI's safe legacy-mode
projection now constructs native filesystem/network permissions directly, then
requires semantic filesystem equality and exact network-policy equality. An
extra writable `.git` root must fail that projection; it must not be lost while
converting a current native profile to a remote session's legacy sandbox mode.

No ALab caller of the removed private `sandbox_policy()` methods or
`ThreadStartedTraceMetadata` struct literal was found. Preserve ALab's startup
account epochs, request-recovery hooks, local-only permission defaults and native
restore admission in the surrounding changed files. Do not restore obsolete
legacy helpers or infer a weaker policy from a diagnostic label.

Require `sandbox_mode_preserves_workspace_metadata_at_the_destination`, existing
non-cwd writable-root/network mismatch/external-profile refusal tests,
`session_configuration_apply_permission_profile_accepts_direct_write_roots`,
thread trace lifecycle tests and diagnostic label coverage. Preserve ALab
`native_resume_permissions_are_scoped_and_preserve_explicit_choices` and
`child_restore_applies_current_approval_policy`, including fresh destination
metadata restrictions and explicit launch choices. Check trace consumers against
the renamed field; no ALab consumer requiring a compatibility adapter was found.

## Removed build/release paths: `ba3071a66d`, `9e48702b3a`

Adopt the retained `third_party/v8/README.md` documentation update in
`ba3071a66d`: it removes the obsolete explicit BuildBuddy cross-build command
and points to the upstream workflow's approved execution environment. Its other
two paths, `.bazelrc` and `codex-rs/docs/bazel.md`, remain wholly absent under
ALab's removal policy. No V8 archive pin, Cargo dependency or runtime code
changes. The retained README does not authorize a new build service or lane.

All three `9e48702b3a` paths belong to removed `.github/` automation, so that
commit is superseded by the existing removal policy. The upstream action is
opt-in, keeps Azure for tooling/certificates/notarization, requires signing mode,
pinned certificate and fresh GitHub OIDC, validates endpoint/purpose/key ID and
refuses an Azure-only provider lacking its signer-service markers. Those source
checks do not certify the external provider or signing service. Carry none of
that automation, credentials or activation into ALab's native atpkg lane.

Require exact removal-policy coverage and post-merge absence for those five
removed paths, retain the V8 documentation update, and leave ALab's certified
application/package gate unchanged. Do not run removed GitHub/Bazel workflows as
qualification or claim upstream release-signing coverage.

## Transparent Linux TCP: keep `7194b21776` opt-in

The Linux helper's `--transparent-tcp` and executor's
`--linux-sandbox-transparent-tcp` are explicit switches. The executor runtime
defaults to false with no TCP upstream. Trusted executor startup may separately
set `--tcp-upstream-proxy` for mandatory HTTP CONNECT routing of opaque managed
SOCKS traffic. Request or repository configuration does not set either startup
option. No ALab literal or direct caller needs the new fields, and intake does
not enable them.

The launcher requires managed networking, trusted proxy ingress ports, system
bubblewrap, private user/network/PID namespaces and private procfs. It refuses
legacy Landlock, inherited host PID namespace and `--no-proc`, and does not use
the ordinary procfs fallback in this mode. Only the trusted setup owns the TUN
and resolver descriptors; the workload drops them before exec and waits for the
relay's real native handshake. The inner stage verifies effective/permitted
capabilities are empty. Route enumeration is permitted through NETLINK_ROUTE,
but mutating the parent-owned network namespace still requires authority the
workload lacks. Existing filesystem and Unix-socket restrictions remain.

DNS gives at most 8,192 stable, never-reassigned synthetic name identities across
IPv4/IPv6 and preserves the original hostname in the SOCKS request. Unsupported
or malformed queries cannot allocate names; unknown synthetic addresses fail.
Resolver mounting checks both the logical `/etc/resolv.conf` and its resolved
target against read denials. The stack admits opening TCP flows, rejects
fragments/unsupported packet forms before allocating listeners, and delegates
protocol validation, sequence tracking and retransmission to smoltcp. There are
at most 128 native flows with fixed receive/send windows and bounded connection
workers. General UDP, QUIC and ICMP are outside this TCP mode; no general direct
network route is created.

Before requesting proxy approval, the relay pins both native endpoints and their
socket inode in the private procfs table. Pending approvals poll ownership in
bounded shared snapshots; final close cancels that socket's request while a
write-half-close keeps the receiving owner live. Normal framing timeouts remain
separate from the trusted proxy's approval deadline. Closing one client must not
cancel another client's pending connection to the same destination. Relay failure
terminates the namespace workload rather than continuing without its data path.

Attribution remains on the trusted host bridge. All upstream streams come from
the managed SOCKS result after policy evaluation. Mandatory TCP upstream errors
do not fall back to direct dialing; an enforced managed context with a mandatory
upstream but no executor-local proxy is refused. The upstream proxy must enforce
the address it dials. Preserve current ALab source custody, native destination
policy and command-tree teardown; the new helper is not a grant to bypass those
checks or transfer old proxy authority into another account.

Require non-skipped native Linux execution of
`managed_tcp_native_client_uses_attributed_socks_and_preserves_restrictions`
with actual system bwrap and accessible `/dev/net/tun`, including hostname and
literal IPv6, forced AAAA lookup, independent same-destination cancellation,
attribution, policy refusal, host-loopback isolation, read-only filesystem and
route-mutation refusal. Its device/namespace skip is not qualification. Also run
`transparent_tcp_rejects_incompatible_options_and_untrusted_proxy_ports`,
`denied_socks_connect_never_returns_an_upstream`,
`both_families_retain_the_original_name_and_literal_ips_are_separate`,
`capacity_exhaustion_never_reassigns_an_existing_address`,
`malformed_and_unsupported_questions_cannot_allocate_an_alias`,
`minimal_resolver_mount_respects_explicit_denials_and_symlink_targets`,
`a_reset_on_either_side_releases_the_bridge_permit`, network/x32 seccomp controls
and `linux_helper_uses_effective_managed_policy`. Preserve
`socks_tcp_waits_for_http_connect_and_preserves_the_tunnel` and
`remote_dns_requires_policy_independent_of_the_resolved_ip`, adding real
upstream-failure/no-direct-dial, helper death, timeout, half-close and flow/DNS
saturation controls.

Carry upstream lock resolution and Linux-specific etherparse/smoltcp pins rather
than edit lock entries manually. Inspect the actual resolved Linux feature graph,
including the moved production Hickory dependency, against prior DNSSEC review
conditions. The 0BSD exceptions are scoped to managed 0.8.0 and smoltcp 0.14.0.
Keep the four Bazel module/lock/build/fixture paths absent. A macOS-only gate
cannot establish runtime behavior of this Linux-only implementation. No Linux
build, namespace experiment or performance measurement was run for this review.

## Endpoint-filtered plugin recommendations: adopt `a46c5be77e`

The recommendation request adds `includeInstalled=false` to the complete routed
GLOBAL query URL. Endpoint results are no longer filtered against loaded plugins
or the local installed-plugin cache. Explicit disabled-tool and client eligibility
filters remain. The two preparatory `plugins_for_config` awaits used only for
that old installed filtering are removed, including the unconditional work before
the tool-suggestion gate. This is a source-path reduction, not a measured latency
or filesystem improvement.

If the endpoint returns an installed plugin, its recommendation now survives
local installed-cache filtering. Do not restore that cache filter silently;
retain actual installation state and ordinary install/admission checks at the
mutation endpoint. A suggestion never grants execution, installs a plugin or
proves account eligibility. Preserve current-source credential routing and cache
ownership. ALab's `plugins`, `apps` and `tool_suggest` packaged defaults remain
disabled; neither intake nor this endpoint query enables them.

Require `recommended_plugins_requests_uninstalled_codex_suggestions` to observe
the complete routed query URL,
`recommended_plugin_candidates_use_endpoint_results_and_filter_disabled_tools`
with an installed local-cache entry, client filtering and recommendation-cache
stale-completion/account-switch tests. Retain the remote plugin install/router
refresh integration cases, which now verify actual installed routing rather than
asserting that a cached suggestion disappears. Add an ALab defaults-off control
for no recommendation request or recommendation-only plugin load, and preserve
retired-source/destination refusal on explicitly enabled recommendation traffic.

## Validation scope

Require exact 82-commit coverage, unchanged first 76 decisions, one active
decision per covered commit and direct historical supersession. The next commit
`f579388d521334532811bcb30fe1046b429dfac9` remains outside this review. All named
native tests, integration adaptations and certification remain pending here.

Observed validation: the maintained intake validator accepts the exact
82-commit record. Source/record checks find exactly one active decision per
covered commit, unchanged first 76 decisions and unchanged bodies in all 15
historical records apart from direct supersession metadata. The 83rd commit is
rejected as incomplete. Private Git-index policy checks cover all nine removed
paths while retaining the V8 README. All 58 upstream workflow Python tests
passed in 24.242 seconds with managed Python 3.14, and `git diff --check` passed.
No Rust command, application build, Linux runtime probe or native qualification
was run for this intake extension.
