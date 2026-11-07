# Protected gate after native source-use composition

Decision: run the complete canonical gate after main incorporated the native
account-source lifetime changes. The earlier passing `f17975bed6` snapshot did
not include that runtime code. The owned peer checkout was detached at
`451dc29941868e9c481f738c16e2ff29ab8bb2ad`, matching fetched origin/main, and
remained fixed throughout the check. The real producer remained fixed too.

Normal `bash alab/check.sh` completed **exit 0**, independently recorded in
retained block 38 of owned m157 shell `s-a845a19f856134e74492`.

- The CLI built and ran (`codex-cli 0.0.0`).
- All ALab targets passed: **1,747 passed, zero failed, four ignored**, over
  25 target reports.
- Tippy found no diagnostics in ALab's code and set aside 41 upstream diagnostics
  under the maintained attribution rules.
- The hooked-library unit phase completed. The local/remote compaction regression
  explicitly passed. Seventeen other failures persisted in isolated reruns and
  all failed on pristine upstream `3026a8e54e0f48c4d8074cd700f35b0697abc329` too;
  they were therefore non-gating. This is a passing attributed gate, not an
  all-green upstream test claim.

The same real `atpkg-publish` from aterm source `08d1cd2d3b` authenticated and
leased pinned Trust 15698. The managed Python path, four build jobs, four test
threads, private Cargo home and existing containment were retained. No compiler
override, native-target override or boundary relaxation was introduced. These
are unverified-lane tests, not a formal Trust verification proof.

The complete tee log on m157 is
`/tmp/.ubd/codex-native-f0a4e96f.8bYgIz/full-gate-native-source-451dc29941.log`,
2,459,347 bytes, SHA-256
`8eaa6ad376199af82be4257ca681afee590c44a3c5aaaaffbf2a8054e94effac`.
Block 39 read the complete log for these totals and its checksum. Terminal
history alone loses early output after TUI redraws, so it was not used for the
whole-run counts. All gate and inspection commands have completed.

Scope: this qualifies the exact source above. The private native source-use
[contract](../design/accounts-source-use.md) still requires complete consumer and
native-controller qualification before adding its SourceV1 image mark. This
source gate does not establish that operational qualification; no capability was
enabled or published here. The signed package channel still selects old build
2026110601/index 101 on this Mac, so runtime adoption remains deferred. A restart
into that same image would not adopt the source fix.
