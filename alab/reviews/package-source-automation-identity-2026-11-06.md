# Automated package-source reflog identity

A standard no-promotion package attempt for gate-tested fb725e80a3 stopped during
source custody on m157: Git could not resolve GIT_COMMITTER_IDENT. Native compiler
selection and its compiler probe had passed; the product build had not started.
Both stable and fresh source reservations were retained, including
~/Developer/codex-alab-pkg-fb725e80a.9N7JT1 and .3bCIMI.
Command-local caller Git identity did not cross the native environment filter.
No global or repository user identity was configured, and the filter was not bypassed.

Decision: source custody owns these worktree-reflog writes and should identify
its automation directly. git_identity now supplies ALab package source custody
<package-source@invalid> both when resolving the exact identity/time and when
performing the corresponding worktree creation or checkout. It still pins Git's
resolved time and validates the exact expected reflog transition and other
metadata. It does not author source commits or modify repository/user settings.

The new regression removes local user.name/email, disables auto-detection,
clears inherited Git environment, and isolates global/system config. Its control
proves Git cannot resolve an ambient committer. Before the fix it failed with
Committer identity unknown. After the fix it passes initial stable creation,
reuse at another commit, and fresh creation, while preserving the driver's HEAD.
Focused local managed-Python/Git run: one passed in 1.980s.

Full standalone package-source suite on exact bd244ab2f4 passed on m157:
27 methods, 26 passed, one existing opt-in native-cache test skipped, 111.721s.
Owned shell s-25205fe33e907f1d5dc8 block 2 completed exit 0. Existing hostile
birth/ref/checkout/reflog mutations remain refused with evidence preserved.
No Rust compiler was invoked by this standalone fixture run. Complete log:
/tmp/.ubd/codex-native-f0a4e96f.8bYgIz/package-source-automation-identity.log.
Whitespace checks passed. Actual package staging will be retried separately;
this result does not claim an installable artifact or installed adoption.

## Actual native staging retry

After landing the fix, the actual no-promotion staging command used packager
bd244ab2f4 and exact gate-tested product source fb725e80a3. Native selection and
source custody passed with no caller Git identity or user-config change, and
release compilation progressed through core, app-server, TUI and exec libraries.
This confirms that the operational identity blocker was removed.

The session monitor subsequently reported job=stuck after more than five minutes
without observed progress. Disk and syspolicyd status were normal. Broader remote
diagnostics had previously been refused, so no alternate process-inspection
route was used. Decision: interrupt this owned attempt rather than infer success
or a source failure. Retained block 3 completed exit 130. This is an inconclusive
artifact build, not evidence of a source regression or a completed package.
The monitor classification alone does not establish the underlying wait cause.

Block 4 read the complete retained log and confirmed the clean packager checkout:
/tmp/.ubd/codex-native-f0a4e96f.8bYgIz/package-opening-fb725e80a3-custody-fixed.log,
72,941 bytes, SHA-256
9402ecadec9ba66aa83ce74d1731f758e28e6f395d8e824d577340842092b0a9.
No completed alab-build.json exists in the dedicated output's package leaves.
Cache, reservations and evidence remain intact. No promotion, install, publication
or restart occurred. The idle shell's status still mentioned a descendant of the
attempt after interruption; without an admitted identity-fenced cleanup token,
no bare-PID signal was sent. Cleanup/liveness remains unresolved, so do not start
a duplicate package attempt or claim all descendants have stopped.

## Scoped custody follow-up

A read of the dedicated cache's own records (block 5, complete exit 0) found its
coordinator lock still held and the third attempt's source guardian still Active.
A subsequent 20-second flock wait also ended with the lock held (block 7,
complete exit 0). This is application custody evidence, not broader remote process
inspection. The supported guardian retains that lock until its leased command
group retires; no receipt was edited, lock replaced, or unknown source adopted.
Decision: keep the shell and evidence, defer duplicate staging and cleanup while
custody remains unsettled. Do not infer process death from the shell prompt.

Meanwhile, the unchanged cache-budget orchestration suite passed all ten methods
in 46.942s on exact bd244ab2f4 (same owned shell, block 6, complete exit 0).
It uses real package/guardian shell orchestration and an inert native authority.
Coverage includes held locks, fresh attempts, failed-build status preservation,
unknown/unadopted outputs, source-family identity, preservation markers, admission
refusal, untrusted guardian parents and refusal to settle unfinished guardians or
refused source returns. No production cache was maintained by these fixtures.
Full log remains alongside the other remote logs as
package-cache-budget-after-interrupt.log. No implementation change was required.
