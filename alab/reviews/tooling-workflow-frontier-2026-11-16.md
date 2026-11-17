# Fixed-source tooling sweep and native prerequisite frontier

Source: `704685627225cb47c9a952add33c1a34c72f99ab`, clean isolated worktree.
Interpreter: managed CPython 3.14. No source changes during execution.

Decision: attempt the maintained native-first Python workflow, retain its exact
failure frontier, and advance the available portable tooling checks. Keep these
selected results separate from full workflow or application qualification.

The configured native project entry returned 1: a qualified `atpkg-publish` was
not available through its configured entry (`ALAB_ATPKG_PUBLISH`/PATH). The native
test adapter was also absent at its default worktree path. No compiler started.
`alab/python_tests.py` discovered 1,367 cases and failed at native class setup,
with zero test bodies executed, one fixture error, and `full_scope=false`.
The pure phase started no cases. Full workflow execution is deferred until its
existing native prerequisites are available; this is not a passing workflow.

Available checks ran independently with ordinary unittest discovery and all
passed, with no skips, errors, expected failures or failures:

| Pattern | Tests |
| --- | ---: |
| `test_sync*.py` | 93 |
| `test_delta_topics.py` | 30 |
| `test_feature_inventory.py` | 27 |
| `test_python_tests.py` | 33 |
| `test_github*.py` | 31 |
| `test_qualification*.py` | 79 |
| `test_upstream_status.py` | 16 |
| `test_upstream_intake.py` | 9 |
| `test_lint_owned.py` | 12 |
| Total selected | 330 |

Each selected command was `python3.14 -m unittest discover -s alab/tests -p PATTERN`.
The maintained workflow used `python3.14 -I -B alab/python_tests.py`. This report
adds evidence only; no runtime, compiler admission or workflow semantics changed.

Evidence remains in the common Git directory under
`alab-validation/704685627225cb47c9a952add33c1a34c72f99ab/python-workflow-20261116/`.
`audit.json` records exact interpreter and commands, statuses, counts, timings,
and SHA-256 hashes for every log. Its own SHA-256 is
`a1997b47ab8fbca1d3b496f86ed759034e5e5127edb7a09a914222eef67c749a`.
Do not extend these results to later source revisions without assessing changes.

Publication composition: remote main advanced to 97a581225c with partial source
status observation changes. Merged that history and passed all 24 upstream-status
tests on the combined tree. The 330-test sweep above remains bound to 7046856272;
it is not a sweep of the later status implementation.
