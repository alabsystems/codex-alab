<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve unknown request metrics

The request-shape diagnostic reported `tool_bytes=0` for generic JSON bodies
that contain tools. That path does not retain a separately encoded tools value,
so zero was not a measurement. It could misdirect investigation toward missing
tool definitions after a backend rejection.

Use an optional byte count. Zero means the tools field is absent; omit the field
when its encoded size was not measured. Count generic arrays directly, without
parsing schemas or serializing their content again. Byte counts describe the
encoded JSON value before compression, excluding its field name and separators.

| Request representation | Tool count | Encoded tool bytes |
| --- | --- | --- |
| Typed request, tools absent | 0 | 0 |
| Typed request, raw tools present | unmeasured | exact raw JSON length |
| Generic JSON, tools absent | 0 | 0 |
| Generic JSON, tools array present | array length | unmeasured |
| Generic JSON, other tools value present | unmeasured | unmeasured |

For example, a generic empty array has a known count of zero but an unmeasured
encoded size; a retained raw `[]` has two encoded bytes. Unknown fields are
omitted in tracing, rather than formatted as a debug `Option` or a false zero.
Request generation, transport, recovery policy and content retention are unchanged.

## Validation

All 257 API library tests passed with source-selected Trust 15698 in the
explicit unverified test lane. The actual HTTP endpoint regression verifies
present raw JSON, present generic arrays, absent fields, empty arrays and null
values while retaining activation, status classification and privacy controls.
Focused Tippy for every API target passed; `alab/lint.jq` reports no owned,
hook-spanning, failed-target, missing-script or upstream findings.

The existing application certification blocker is unchanged; this diagnostic
correction adds no dependencies or application-package qualification claim.
