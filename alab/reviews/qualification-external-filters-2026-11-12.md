<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Compare source without external clean filters

Base source: `8f1a4e47552aac9ad22f04e04f7e4e5b1c248730`.
The audit and validation used temporary Git repositories and inert shell gates.
No live gate, provider account, installation, or native build was used.

## Reproduced problem

A committed `alab/check.sh` exited 17. Its working copy was changed to exit zero,
while a configured clean filter returned the committed bytes to Git. The fresh
index comparison reported a clean candidate, then qualification executed the
altered working gate and wrote a successful record. `inspect()` accepted that
genuine generated record. No record forgery was needed.

The failing regression was run before the implementation change:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_qualification.py -k external_clean_filter -v
```

It failed because the altered gate was not refused before execution.

## Decision

Enumerate configuration keys only, with NUL framing and a 64 KiB limit. For each
configured external filter driver, pass command-local empty `clean` and `process`
commands and `required=false` to the fresh working-tree comparison. This follows
the existing removal safeguard in `package_source.remove_fresh`; it changes no
repository configuration. Empty or ambiguous override names refuse the
comparison instead of falling back to filtered evidence.

Each recursive submodule comparison obtains its own filter overrides. The
existing Git text and line-ending rules remain active: disabling external
commands does not disable built-in EOL normalization. Index construction and the
committed-index comparison retain their existing behavior.

Qualification records now use schema **4**. Earlier schemas cannot establish the
new comparison guarantee and remain historical observations; `inspect()` rejects
them as evidence for `qualified_sha`. This remains local execution evidence, not
authenticated provenance or an atomic defense against unrelated concurrent
configuration changes.

## Validation

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_qualification.py -v
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_upstream_status.py -v
git diff --check
```

- All **35 qualification tests passed** in 29.385 seconds.
- All **15 related status tests passed** in 11.622 seconds.
- Diff whitespace check and independent code review passed.

The new cases reject the filtered altered gate before either gate marker can be
written, reject a filtered submodule change, and qualify unchanged source despite
configured external clean/process commands that would fail if executed. They
also preserve built-in CRLF conversion, support a configured driver name with
case, dots and whitespace, reject unsafe override names, and reject oversized or
malformed key records without reading configuration values. Existing clean-source,
hidden-index, root, recursive-submodule, gate-exit, and historical-record checks
remain in the passing suites.
