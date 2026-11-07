# Native source-use controller prerequisite audit

The passing client source gate does not establish SourceV1 activation readiness.
Read the controller's actual private request parser at a fixed remote commit,
`alabsystems/aterm@bc8b3c72e1fd3301c669bb65a2d76e1a50945bfa`, using Agh.
This is newer than the frozen 08d1cd2d3b producer used for client build admission.

That parser accepts member-use-begin, member-use-end and member-use-close, with
strict nonce, member and capability parsing. It still recognizes only member-open
as omitting the common member field. Its closed action match has no
member-open-derived, member-refresh-use or member-retire-owned action; unknown
verbs return None. Therefore this exact controller revision cannot serve the
complete [client contract](../design/accounts-source-use.md). These are protocol
admission gaps, not evidence that an enabled production capability is broken.

Decision: preserve the existing activation prohibition. Do not mark the client
SourceV1 image, replace these actions with legacy verbs, treat unknown responses
as Closed/Ready, or infer controller readiness from SDK protocol tests. The next
implementation/qualification work must include the typed derived-open and
contextual refresh routes and owned attempted-preload retirement, with their
actual caller/image/App-scope/source custody checks and cancellation/replay
semantics, before complete consumer/controller qualification can permit activation.
No controller code, native policy or capability was changed during this audit.
The current client tests already cover exact retirement metadata and refusal of
unknown retirement reply fields; the full source gate is recorded separately.

## Current-main recheck

Read the exact current controller source again at
`alabsystems/aterm@72129d56c20592134861c271044f0640d945588a`, through Agh.
The retained codex_source_control.rs is 42796 bytes, SHA-256
9f08ed8f27a8311a564dc85b4007f892d24f3d9688842b897905b20d8e88b887.
Request::parse still permits omission of the common member only for member-open,
and its closed action match still returns None for unknown verbs.

The complete contract comparison identifies six absent admission routes:

- member-open-derived
- member-preload-use
- member-ready-use
- member-reader-ready-use
- member-refresh-use
- member-retire-owned

Member-use-begin/end/close remain present. The contextual preload and Ready
routes are additional gaps beyond the three highlighted in the first audit.
Recognizing only their parent-use setup cannot admit the complete consumer
protocol. This source inspection establishes a parser/front-door prerequisite;
it is not a new runtime rejection test, a controller qualification verdict, or
evidence of failure in an activated SourceV1 deployment.

Decision unchanged: implement and qualify the complete typed routes with actual
kernel caller/image/native-instance, App scope, source-family/use tenure,
cleanup and replay semantics before considering SourceV1 activation. Do not add
parser-only acceptance, synthesize authority, or substitute legacy actions to
make a smoke pass. No native source, policy, image mark or installed selection
changed in this recheck. The opening-policy fix and its deferred rollout/package
adoption are a separate workstream; this capability audit does not add an
activation prerequisite to that greeting fix.
