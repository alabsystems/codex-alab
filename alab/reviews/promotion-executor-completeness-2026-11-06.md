# Promotion requires the declared execution server

Review after artifact staging found that promote.package validated codex,
code-mode-host and ripgrep but ignored the manifest's execServer member.
A published artifact could lose its execution server after the package smoke
and still pass reuse/promotion validation. A malformed or redirected execServer
declaration also reached the selector. Two regression methods reproduced this:
one missing-executable failure and five malformed declaration failures.

Decision: when a manifest declares execServer, require the canonical Darwin path
bin/codex-exec-server and include it in the common executable validation. It must
be nonempty, regular, executable and protected from group/other writes; a link
does not count. The fixed bin parent already undergoes regular-directory checks.
This adds no execution or filesystem mutation to metadata validation and does
not change selector guards, source ancestry checks or native authority. Historical
manifests without this optional component continue through their existing layout
checks; inventing a companion they never declared is not part of validation.

The new tests use inert private fixtures and a recording selector. Missing,
truncated, nonexecutable, writable and linked executors are refused before any
selector call. A complete executor remains eligible. Null, boolean, array,
another packaged binary and parent-traversal declarations are refused.

Verification: promotion 20/20 passed in 4.161 seconds; local update 16/16 passed
in 8.538 seconds. The complete scheduled-job suite ran 37 methods in 53.174
seconds: 36 passed; its historical-bootstrap method had two subcase errors
because the expected status record was absent. Running that exact method on
unchanged main 3cce87d652 in an isolated worktree reproduced both errors in
0.562 seconds. This is an inherited fixture failure, not a passing suite or an
effect of the executor validation. It remains a follow-up diagnosis. Diff
whitespace checks passed. No Rust
source changed, no real package was selected, and the separate remote rollout
measurement remains unverified under the recorded fleet refusal.

Follow-up diagnosis of the inherited historical-bootstrap test found stderr
reporting Apple's unavailable Python launcher and failure to look up the account
home. That precedes the controller handoff the test intends to exercise. Its
copied historical lib.sh now binds only that fixed account-home query to the
fixture interpreter (shell-quoted for its spaced path). The test asserts exactly
one query is replaced; all historical guardian/lifetime code remains intact.
This is a fixture adaptation, not a change to deployed historical sources or a
permission workaround. Both historical bootstrap subcases now pass in the
focused method (1.176 seconds), including the retained-controller audit record
and refusal to build or select. The clean baseline worktree was removed after
the comparison.

The complete scheduled-job suite then passed all 37 methods in 53.551 seconds,
with no skips. The fixture correction resolves the inherited errors described
above; the original failure and unchanged-main comparison remain recorded.
