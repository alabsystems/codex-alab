# Lint fixture import and nested containment

Decision: correct the remaining test-local sibling import so the lint-step suite
can load under isolated Python, using the same exact-file import pattern as the
native runner suite. A source search found no other `from test_` or `import test_`
statements in alab/tests. The loader reads only its known sibling; it does not
extend sys.path or relax isolated mode.

An isolated managed-Python probe first executed the committed pre-change module:
it raised ModuleNotFoundError naming test_unit_results. The corrected module
loaded successfully and unittest registered all 38 cases. Its direct isolated
`--help` invocation also exited 0. These are import and registration checks,
not runtime passes. Diff whitespace checks passed.

The separate earlier nested-profile failure remains accurately deferred. Source
inspection confirms alab_contained allocates a fresh /tmp/alab.XXXXXX directory
and validates that exact namespace; check_main also requires admitted scratch.
The earlier outer profile admitted only its allocated scratch, so nested calls
cannot allocate sibling roots. Changing TMPDIR in the fixture cannot satisfy
that contract. Broadening the outer profile or adding a production scratch
override solely to make these fixtures green would change the boundary being
tested and is not justified by this import correction. No containment rule,
scratch allocation contract, runtime Rust source or installed selection changed.
The prior 31 passes, five failures and two errors retain their original scope;
the full 38-case suite was not rerun under a different boundary.
