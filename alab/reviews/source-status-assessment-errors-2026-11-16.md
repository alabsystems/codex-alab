# Preserve source observations when intake assessment fails

Decision: after source observation completes, retain it if optional assessment
cannot read its candidate, catalog, intake records or requested range. Append an
assessment error, keep the stderr diagnostic and return status 1 with partial
JSON. Do not invent an assessment or convert the error into success. Invalid
invocations that fail before observation still return an ordinary CLI error.

Evidence: five real CLI cases failed before the change by discarding completed
source JSON: missing candidate, absent candidate catalog, missing intake file,
malformed JSON and unknown base. A controlled timeout escaped the handler. All
now preserve main/upstream/frontier and pending count, omit the unavailable
assessment and report an error. KeyboardInterrupt and SystemExit still propagate
with their original identity. The no-base intake invocation remains invalid and
the existing supported catalog-schema diagnostics remain intact.

Validation: all 27 upstream-status, nine intake, 27 feature-inventory and 18
artifact-status tests passed (81 total), plus diff whitespace checks. These are
local tooling tests, with no native workflow or application qualification claim.
