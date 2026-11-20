<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve qualification exits through broken standard-stream shutdown

Hostile review of landed `9cb571c212` found that injected notice EIO tests
did not exercise Python interpreter shutdown. A real readerless stdout pipe,
installed after an actual gate wait returned 23, let the original CLI select
SystemExit(23), then Python retried the pending buffer and exited 120 instead.
The same failure affected a broken stderr while reporting an observation error
and could replace a pending SystemExit(47). KeyboardInterrupt retained its
native signal exit but lacked the required output-quiescence control.

Decision: keep the library `run()`, `main()`, `write_record()` and `inspect()`
unchanged. A small executable-only `cli()` boundary flushes both standard streams
before returning or re-raising. Only a BrokenPipeError from that stream's actual
flush permits redirecting its descriptor to the null device. An unrelated
record/hash EPIPE cannot redirect a healthy output. The separately opened null descriptor is
closed; if allocation itself occupies the target descriptor, ownership stays
with that output. The selected nonzero status survives ordinary cleanup errors,
and the existing priority retains the exact pending KeyboardInterrupt or nonzero
SystemExit.
A successful return or parser help exit cannot hide a failed output flush.

Tests-only commit `14f11fa23fa39cc643e730af8650fd5820c59d55` measured the old
production before applying the fix. All 18 selected methods finished:
15 passed and three failed-subtest, with 12 assertion failures, no errors or
skips. Broken stdout and broken stderr each failed four cases: actual process
exit 120 replaced expected 1, 23, 75 or 143. Two pending-SystemExit(47) cases also exited 120. The two KeyboardInterrupt
cases already exited -2; they failed only the missing-quiescence witness.
Do not classify those two cases as status masking. The original two CLI entry guards were executed; witnesses distinguish
the real wait, selected SystemExit and final native process exit.

The added three-method combined-record-cleanup suite passed before the fix,
across seven controlled combinations. It witnesses complete real JSON/flush,
physical writer close, then an independently failed temporary unlink.
Primary EIO remains exact over cleanup EIO, a new cancellation can supersede an
ordinary primary with its cause, and an existing cancellation stays authoritative.
This closes the previous report's unmeasured combination; it is regression
coverage of existing behavior, not another demonstrated production defect.
The unrelated hash-EPIPE/no-redirection control also passed on the old code.

The fix is `fe56a7afd9`; `f78680c7a0` adds four direct CLI policy methods.
These unit controls use flush-only owned callbacks to cover returned statuses,
help output, wrapped success-shaped exits and cancellation priority. The actual
pipe regressions use private Git repositories, finite committed shell gates,
real waits and OS pipes whose reader has closed. The pipe controls retain real wait, close and publication operations; they do
not substitute a successful child status.
The first fixed-source run at `f78680c7a0112f15af3d95ce9a100303e77c9b6d`
passed all 161 methods. A separate fault probe then showed an additional error
path: failed null-device allocation left the pipe buffer retryable, again
changing selected 23 to native 120. A second regression therefore requires
closure through the already-failed Python stream object when quiescence fails.
The close can re-raise EPIPE after marking the IO object closed; that ordinary
cleanup error must not replace the selected status or pending cancellation.
The fallback does not raw-close a standard descriptor. A standard FileIO can
retain its native descriptor until OS exit; the claim is preventing Python's
buffer retry, not establishing EBADF or native descriptor closure.
The fallback is `87545b7e38`. The second tests-only source
`8656110616ea4b66a1f95066ea22d54f8c764601` finished both null-output methods
with 12 assertion failures: ten actual native-120 status mismatches and two
KeyboardInterrupt controls that already retained -2 but lacked IO closure.
All 12 witnessed an attempted null allocation and an open failed IO object.
The six-line fallback was applied only after that run closed.
The final run on `87545b7e383c79df7fe4b7181bd861c31764ca63` passed all 163
unique selected methods: qualification 122, upstream status 27 and daily sync 14.
All original processes, guardians, named-result collectors and source checks
completed with status 0, with no failure, error or skip. The final source/tool
book `b85165a43079f616c1c62c1a448ab48a141755225e5366617809ce905aaf73d7`
bound all 1,088 tracked ALab paths and 12 tool/helper paths unchanged across the
run. Library-function ASTs remained identical to the initial landed source.

Final packet: `codex-qualification-null-green-87545b7e-ve689h49.noindex`;
`closed-summary.json` SHA
`f9447aa7b9f48a8f9d216195b82edc7e300a7a7f5ccb9d65760ce7e73c1e7671`.
The earlier sealed RED records are unchanged. Their classification correction,
`earlier-red-classification-erratum.json`, SHA
`eae4a3d59422d5027d0defa877cbdef7498605bba1660f227dfd15f0b91c4c88`,
preserves the difference between ten status mismatches and two KBI cleanup
controls in each RED phase. The second RED packet is
`codex-qualification-null-red-86561106-sy2ouddp.noindex`, summary SHA
`2faad42f42b1562b613d490f9b05d8d50b279a1640e79f16837a7a7b1e9e29cf`.


RED packet: `codex-qualification-hostile-red-14f11fa2-4btum7au.noindex`;
`closed-summary.json` SHA
`c4c76a8b17dd41437014a85de93add69995fb3a96db72ecce2d610ee90aac232`.
RED source/tool book
`b3437ea7e9639bc8f2277c4287c65138dfe02893abc2921d55d2738bc94cb5b8`
bound 1,086 tracked ALab paths and 12 tool/helper paths unchanged during the run.

Independent review approved the source boundary and descriptor lifetime.
The checks establish local Python CLI behavior. They do not run or certify a
native application, promote a build, or replace the earlier native gate and
host/image prerequisite outcomes with a pass.
