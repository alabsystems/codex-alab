<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native fixture timeout evidence — 2026-11-15

Decision: retain partial Cargo output when the native-context fixture times out,
preserve that timeout as the primary exception, and keep its command and deadline.

The full Python workflow at `214a6704c13a83e0a6d3b1fb49b23b3296dacf9a` ran
1,045 tests in 1,488.001 seconds: zero test failures, one fixture-setup error and
ten skip entries (nine whole tests and one subtest). Both exits were one.
NativeRunnerTests.setUpClass timed out after 60 seconds at the native admission
entry for the
`--unverified test --offline --locked --no-run --lib -p alab-native-context`
command. Its three mandatory native-context tests did not execute. The source
stayed clean and the support-runner hash stayed unchanged. This is a failed
workflow, not full qualification. The eleven-file archive has record SHA-256:
`df2fef00dd786174fa99a17d81d395461ddd47456c4f55b019dc9fa6e5fbc9ac`.

The fixture saved output only after subprocess.run returned. TimeoutExpired's
partial output was therefore lost. Other jobs reported resource admission waits,
but the lost stderr prevents attributing this timeout's exact cause to them.
The new capture closes that independently reproduced diagnostic gap; it does not
resolve the earlier timeout's cause or turn an unrun native test into a pass.

Every completed command keeps the original argument/status JSON and byte output.
A timeout now records status null, outcome timeout, its timeout_seconds, and the
available stdout/stderr bytes, including empty output. The identical exception is
re-raised. A capture OSError adds its type and message as an exception note while
preserving the original timeout and its partial output. Command, cwd, environment,
preexec signal callback, admission and the 60-second deadline remain intact.

RED `19c660121244f65bc7279e9d7473584e3ca9f85f` ran four pure methods: three
diagnostic assertions failed, while normal completion passed. There were no
framework errors or skips. Its six-file archive has record SHA-256:
`604e3d3a886118f78d8137ed16d08b6bff4b15ceff356899b89d5250ea56d566`.
GREEN `dda69aebd3be36e08ca8ff3f9c05b57bfa052968` passed all four without errors
or skips. Its twelve-file archive has record SHA-256:
`1ac69e5788cc88fb62b2a45eb08bdc0d3a069f71c44346ed46bb560456bafaa8`.
Independent review verified the records, every indexed hash, non-UTF8 partial
bytes, empty files, exception identity and normal-path compatibility. Default
test fixtures clean up; the explicit private archive parent retains observations.

The four pure controls mock only the subprocess boundary. They supply no native-context
pass, application certificate or installed validation. The failed workflow and
the separate failed certified application gate keep their exact source results.

Two actual focused native-context replays then used clean source
`779eb1cd325b3ab00cf8448598683bfb4e84bcea`, the same source-selected Trust
15698 admission entry, support-runner identity, command and 60-second deadline.
The first ran zero tests and failed setup after 60.154 seconds. This time the
fixture retained timeout JSON and partial stderr: native admission was waiting
behind an exclusive 31.1 GiB reservation. That explains this replay's wait, not
the earlier workflow's lost evidence. Its fourteen-file record hash is
`705b3a7fd3467dcf84f2f64491a3e538237e3ff12c9b16ca345b7af1e8f10cd9`.

After that reservation ended, the second replay passed all three mandatory
native-context tests in 1.363 seconds, without errors, failures or skips. Both
exits were zero. It tested runner preservation/conflicts, triple-runner priority
and missing-receipt refusal, and actual Cargo context preservation/Python-startup
detection. Its forty-file record hash is
`8d0bd679badfd68def06c0fe33fe43acd5a083e41671ab4366fc80f485e59238`.
The support-runner SHA-256 remained
`be3d0f455efdb10f06439bb4d8bb64bf5a281de28405aaa32a933a4809c3f68c`.
These are focused native results; they do not replace the failed full Python
workflow or qualify an application build. Raw context receipts remain private.

The subsequent [complete prerequisite-first workflow](python-native-prerequisites-2026-11-15.md)
closes the full-workflow follow-up at composition
`c5322022b41660536c3dbe818383bfb5014a7e15`: all 1,079 discovered cases ran,
including all three native controls first, with zero failures/errors and eleven
explicit skip entries. That orchestration preserves the current native fixture's
admission, command and 60-second deadline. Its separately retained full result
supplies no application certificate and does not change either earlier failure.
