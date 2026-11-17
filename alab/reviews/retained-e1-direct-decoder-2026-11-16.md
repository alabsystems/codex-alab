<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Retained E1 direct JSON decoder

A real retained E1 frame nests NativeSection raw JSON inside UiData and requested-editor enum variants. Serde's internally tagged Content buffering cannot deserialize those RawValue fields. Decode now keeps the original wire schema but selects a strict string state and parses the original closed variant directly from its JSON bytes. Unknown, missing, duplicate and cross-variant fields still fail.

SelectionData.initial and four UiSnapshot optional fields now require an observed value or explicit null. An omitted initial owner/configuration field cannot silently mean a genuinely observed None. Serialization and existing schema values are unchanged.

Exact input: HEAD7e4be424e857f88edc69cb8872a51c5239d1b815 plus three files, patch96e07a21e2ed3799766e38e1c6187742bbe6e75cf4e19d2f3f71ea70b4081229. Source-selected Trust15698 explicit unverified run2381 passed788 tests across11 affected libraries, zero failures and3 existing ignored. Six new controls passed, including the actual production encode/MAGIC/decode chain for both UI variants, requested raw editor nesting, required nulls and malformed/duplicate variants. The test captures actual PTY mode/identity DATA without writing mode or claiming a terminal/input owner; original loader snapshot completion supplies configuration DATA. Initial run96024 stopped at a Darwin openpty const-pointer test compile error; the corrected mutable-null test arguments are included in the final passing input.

Twelve-package lint completed. The official classifier reports181 owned diagnostics, zero spanning/failing/build-script diagnostics, and20 upstream diagnostics set aside. This is not clean lint. The diagnostic-group count changed because existing transfer helpers are now used in the test target; pending whole-App APIs are not suppressed or removed to hide them.

These controls prove parsing and required-field fidelity. They do not prove an installed App, callback/editor/image ownership, model consumption, Native/Source permission, real signed C1/C2 replacement or shipping certification. The complete ConfiguredApp/StartupDraft scene consumers, original invocation/image custody, full native graph and actual capture/exec caller remain required; no ordinary retained activation occurs here.
