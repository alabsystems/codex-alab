# Status-line inventory and thread-scoped resources

Extend the reviewed prefix from 38 to 41 commits through
`95e0103a5de64e6a097349c37a3f84c42decc8f4`, preserving all prior decisions.
There are 55 later commits in the observed `f941a5fb92` snapshot. This is source
review; native execution and certified integration remain pending.

## Inventory (`91621372af`): keep infrastructure opt-in

Global inventory requests omit a thread ID and retain tool errors and plugin
identity. Targeted replies must name exactly the requested server, with no
additional page. Repeated pagination cursors fail instead of cycling. The
transport filter requires an explicit null `httpOrigin`; missing fields from
older hosts are skipped. As its comment states, null also covers invalid or
unsupported HTTP URLs, so this filter alone is not proof of a stdio transport.

The live embedded-host test covers project overrides, CLI settings and plugin
identity; controlled replies cover pagination, missing transport information,
hosted origins and mismatched targeted names. Require these checks on the
integrated candidate. No picker or automatic provider execution is enabled
by this commit.

## Resolution (`2de2e03d0b`): keep selection opt-in

Tool metadata must advertise exactly one valid status-line `ui://` resource.
The original URI spelling is retained; parser-recovered malformed forms and
multiple valid entries are rejected. Titles and plugin labels are display
data, separate from the saved server/tool identity.

Resolution reads effective configuration layers for the displayed cwd. It
rejects project contributions that remain in an effective MCP server, plugin
or marketplace registration, including partially overridden nested fields.
Disabled layers do not contribute. A saved selection must be advertised
globally; the thread-scoped catalog must retain its plugin owner and a
connected runtime. Resource reads carry the displayed thread ID and resolved
URI, with the supplied snapshot under the dedicated status-line metadata key.
No tool call is synthesized.

The code explicitly cannot distinguish same-name, same-owner registration
replacement, including a reload between lookup and read or a thread-start
override. Do not describe its checks as immutable registration binding. Keep
that limitation visible for activation review, alongside project-layer and
owner tests. The config-reader signature changes have no direct ALab callers
found in the source search; still compile the integrated TUI and glue.

## Streaming discovery (`95e0103a5d`): adopt bounds without enabling it

The global inventory gets a two-second head start; missing known servers are
then probed with at most four concurrent lookups. Completed inventory permits
immediate probes. The first result for each server wins; a late page can cancel
an unanswered probe and free its slot. Inventory remains open to discover new
servers, and server errors remain distinct from inventory errors.

Refresh and selected-provider lookup have independent five-second deadlines,
with timeout winning simultaneous readiness. Dropping the stream cancels the
TUI's waits, while already received host requests may finish. The unbounded
update channel and inventory cardinality are not memory limits; do not infer
one from the request deadline or four-probe limit.

Read all changed source and tests. Require the paused-time races, probe limit,
independent deadlines, missing servers, cursor failure, cancellation/reopen,
URI ambiguity, current-thread resource routing, project layering and plugin
ownership cases. Preserve built-in status-line defaults and existing Apps /
plugin policy. These commits remain staged infrastructure, not authorization
to enable external providers.

## Validation

Check exact 41-commit coverage, unchanged prior 38 decisions, one active record
per covered commit, and rejection of the next unreviewed target. Committed
admission fixtures validate the review graph only, not runtime behavior.
