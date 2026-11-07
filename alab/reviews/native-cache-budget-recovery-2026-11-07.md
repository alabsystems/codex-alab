# Native cache budget qualification, 2026-11-07

Decision: resolve the remaining native cache-suite deferral using the independent
authenticated member already used for native runner qualification. Build the
development atpkg CLI from the frozen producer source; atpkg-publish is a different
executable and cannot substitute for the cache API. No installed executable or
package selection changes are needed.

On permanent peer node-a5ffed2df7233806818dffd88991c0ae (terminal hostname m110),
fresh owned plain shell s-ece24a4f6892c61ec5d4 built atpkg with
`CARGO_BUILD_JOBS=3 targo --unverified build --locked -p atpkg --bin atpkg`.
Producer source ~/aterm-s0-native-runner remained clean at
08d1cd2d3bb07b8ab774abf231ac88f7343b0d05. Block 0 completed with exit 0;
the incremental development build reported 2.37s.

Consumer source ~/codex-alab-s0-native-runner remained clean at
4d1e1bf775d5e608eeacbf63ba320ee62865bbc5. Its isolated Python invocation of
alab/tests/test_package_cache_budget_native.py used the actual development atpkg
and the authenticated physical Trust 15698 sysroot. All four cases passed in
12.338s, no skips; block 1 completed with exit 0. They exercised:

- native policy target reuse across commits;
- registrar compatibility refusing to upgrade old or ambiguous shims;
- v2 package builds retaining default containment and settling;
- refusal of unrelated settlement after actual worker-group retirement.

The existing fixtures compile small real Rust programs in retained private
directories and use the real native cache API. Their compiler namespace is a
fixture value, not a production source attestation. Explicit compiler pairing is
confined to those isolated fixtures. This does not qualify a production source
selection override, installed Codex adoption, the deferred m157 rollout proof,
or SourceV1. No formal proof claim is made for the unverified build lane.

Block 2 completed with exit 0 and independently checked clean source heads and
retained artifacts:

- ~/aterm-s0-native-runner/target/debug/atpkg: 50784264 bytes,
  SHA-256 cc4b59ec5e92dbcf6dda372e0b4e3e1ddfc2d90aa0fb7f365bc0ae8d7da00989.
- Consumer .git/s0-native-cache-budget.log: 1383 bytes,
  SHA-256 80c18d1be7178e9c8033cf634c35429a15391e21c503a528058c1478c09f070d.

Retained fixture suffixes under the host's temporary .ubd directory are
native-cache-budget-g_1xcx0w.noindex, native-cache-budget-8flfzyl2.noindex,
native-cache-budget-wmkndpc7.noindex and native-cache-budget-qxjsw6e5.noindex.
The separate nested lint-profile suite remains deferred; this result does not
weaken its protected profile or reclassify its prior failures as passes.
