# Numeric Windows diagnostics and a realtime test fix

Extend reviewed upstream coverage from 35 to 38 commits, through
`553c764e8b99fa519e9c6eb9b657875ef0f5e1c1`. Preserve earlier decisions.
This is source review only; the observed upstream snapshot has 58 more commits.
Windows execution and the certified application gate remain pending.

## AppX diagnostics (`a46f05e2e3`): adopt

The optional lookup runs only while constructing an existing deployment
failure. It uses a nonzero deployment activity GUID, the AppX operational
channel and error-level events. It reports sorted, deduplicated numeric event
IDs. It does not copy event messages or replace the original HRESULT. Missing
activity, unavailable event logs, render failure and empty results leave the
registration outcome intact. The existing exact-target privilege-error
reconciliation returns before this lookup.

The code requests one render property, supplies one aligned `EVT_VARIANT`,
and checks successful rendering, property count and type before reading its
UInt16 union member. Microsoft documents EventID as UInt16 and describes one
rendered variant per selected property:
[Event properties](https://learn.microsoft.com/en-us/windows/win32/api/winevt/ne-winevt-evt_system_property_id),
[EvtRender](https://learn.microsoft.com/en-us/windows/win32/api/winevt/nf-winevt-evtrender).
The enumeration asks for one handle at a time with zero wait. Query, render
context and each returned event enter `Owned`; the cached `windows 0.58.0`
binding implements `EVT_HANDLE::free` with `EvtClose`, matching the
[EvtNext ownership requirement](https://learn.microsoft.com/en-us/windows/desktop/api/WinEvt/nf-winevt-evtnext).
These checks support the source review; they do not substitute for native
Windows execution.

Require native registration-result tests and event-log cases for a matching
activity, duplicate IDs, irrelevant activities, missing/zero IDs and unavailable
diagnostics. Confirm the original HRESULT/downcast and target-registration
postcondition remain intact. The upstream change adds no injected event-log
test; that validation gap remains explicit. Search found no direct ALab caller
or wrapper of these diagnostic functions.

## MXC launch diagnostics (`818bde0995`): adopt

Extraction accepts only the exact SDK `CreateProcessW` prefix, parses the
substring before the first closing parenthesis as `u32`, and appends that
number to the existing public error. Invalid or overflowing numbers and other
API prefixes fall back to the original message. The extended diagnostic tail
and buffered SDK logger are not emitted. Existing denied-path and permissive
learning policy refusals remain before launch.

The new tests cover a valid error with a private trailing working directory,
nonnumeric text and a different API. Require these Windows-gated tests, plus
overflow/malformed delimiter and actual launch failure checks, on the target
platform. This Mac does not execute the Windows-only native module.

## Realtime helper (`553c764e8b`): adopt

The one-line change adds a semicolon after the diverging `bail!` call when the
event channel closes. The surrounding ten-second timeout, successful event
return and error text remain unchanged. Require compilation and the realtime
request tests with the integrated source-selected compiler. No runtime
behavior change is inferred from this syntax fix.

## Validation

Validate exact 38-commit coverage, unchanged prior 35 decision objects, unique
active coverage, and rejection of the next unreviewed target. Committed
admission fixtures test only the decision graph and supersession records;
they are not a Windows or application qualification result.
