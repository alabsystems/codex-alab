<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Reject a repeated expected-failure callback for one selected case

The workflow tracker already rejected repeated success callbacks but accepted
two expected-failure callbacks for one active pure case. With complete starts
and stops, its underlying unittest result remained successful and the workflow
qualified the inconsistent observation. The new inert regression uses ordinary
`@expectedFailure` and default `TestCase.run`: the body emits one callback before
its expected assertion, and unittest emits the second. It mutates no result
fields and supplies no replacement `run` implementation.

Decision: track whether the active case has already reported an expected failure,
reset that flag at each `startTest`, and record `duplicate_expected_failure` on
a repeat for that same object. Preserve raw counts and ordinary unittest success
as observations; the integrity error makes workflow qualification and returned
success false. Skip, error, fixture, subtest and other outcome rules remain
unchanged. Native prerequisite strictness and ordinary pure expected failures
are unchanged. This is an observation-integrity fix, not a new outcome policy.

The complete maintained inert module runs against the exact
`python_tests.py` Git blob from
`64e4e8271ef831b49fc95bdb9c21543478cfb260`, SHA-256
`13d413f4ec5eed5df3dd97032fcf70463862fa99c018866e2579e645f8dff286`.
Baseline: 33 outer methods, exactly one assertion failure in the new duplicate
regression, zero errors/skips, actual exit 1. Fixed: all 33 pass, actual exit 0.
The 30 existing methods remain passing. Two other new methods verify reset
across separate expected-failure cases and compare real cleanup/subtest/skip
outcomes with a raw default `unittest.TestResult`, including repeated cleanup
skips and errors. All cases are inert, including the fixture prerequisite class;
no actual native prerequisite or complete Python workflow is executed.

Initial exploratory mixed skip/expected-failure negative controls imposed a
broader policy and are withdrawn rather than claimed fixed. Managed CPython's
real default-run controls show that the tested cleanup/subtest skip paths
suppress the expected-failure callback. The retained raw-matched compatibility
controls determine ordinary behavior without adding a blanket one-outcome rule.

The final complete three-module union is shared with the explicit GitHub routing
fix: 21 transport, 10 publication and 33 workflow methods, all 64 distinct outer
methods pass in 18.853 seconds with actual exit 0 and no outer failure, error,
skip, expected failure or unexpected success. The intentionally varied inner
fixture outcomes are not additional passing tests. Selected source, test,
script and Git/Python entrypoint fingerprints remain stable before/after.
Independent review approves the exact guard and its public workflow controls.

Fixed workflow SHA-256:
`be2d03cdc9b2c69b99a2a45c1ead2ed8dca32ae9f162d7a10e8597c69101db22`.
Maintained controls:
`646dc7dc26d9185e5d9a4ea19b9454763a59ea2fcdbe1c9024fd6fd7f343317d`.
The common Git directory retains the shared 35-payload, 415,618-byte packet at
`alab-validation/64e4e8271ef831b49fc95bdb9c21543478cfb260/transport-workflow-kv2iobfy/`,
index SHA-256
`485b4947a92ebd0cf648c1ed716d8ab55549be484e33ecd363d905a2c7b49344`.
Root checks the exact baseline, frozen source/tests, counterfactual and union
outcomes, all payload hashes/sizes and the exact regular-file inventory. The
packet retains the explicit audit correction and real unittest controls.

This changes only the ALab-owned workflow helper, its tests and this decision
record. Disjoint incoming sync work at
`80d42a7e513f1bc1ee108735a945b5e40fc9f789` is preserved without relabeling the
checks as latest-main qualification. No Rust build, actual native prerequisite,
application certificate or full-workflow success is claimed by this packet.
