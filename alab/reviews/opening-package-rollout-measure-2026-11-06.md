# Opening-fix package rollout measurement

Decision: follow the successful private package smoke with the maintained aterm
`lab codex-measure` integration procedure before selecting this development
artifact. Smoke checks alone do not qualify aterm's rollout interpretation.

The original frozen producer checkout refused an agh fetch because its shared
Git configuration enables extensions.worktreeconfig. No configuration was
removed. Following agh's supported recommendation, a fresh isolated clone was
created on m157 at ~/aterm-s0-rollout-measure, branch
alab/s0-rollout-measure, exact main 0e933ed65cdb96acfa5b24f6a924b4b706fd36ba.
The pinned astream a9f174b8565ebc6c58db419d5b73e983da3f8a34 was obtained
through agh, absorbed as a submodule, and selected with --no-fetch. The source
and files were kept fixed. Trust 16387 remained selected by trust.lock.

Owned plain shell s-229412c25ae7dc2f6d1d, block 3, ran
`CARGO_BUILD_JOBS=4 targo --unverified lab codex-measure --package` against the
final artifact documented in package-controller-cancellation-2026-11-06.md,
with pipefail and a tee to
/tmp/.ubd/codex-native-f0a4e96f.8bYgIz/rollout-measure-fb725e80a3.log.
This procedure builds a debug aterm, runs the proof twice, checks reduction
fixed points and privacy, and runs its standing tests. No release binary was
run with a fresh HOME and no installed executable was overwritten.

Observed passing intermediate checks included 14 simultaneous proven supervisor
claims; person input distinguished from control writes; waits on Codex's own
report; continued work while a need stands; the two-minute done-claim backoff;
Off/On and hold behavior; one usage-limit resumption without duplicate requests;
and rejection of Land claims with untracked or unpushed work. A clean Land case
stayed quiet for 150 seconds and reopened on later person input. These are
intermediate observations, not a completed measurement verdict.

During the proof the fleet drive connection dropped. A subsequent supported
wait returned: `ERR denied: this read needs the same driver's untouched plain
shell; not await`. Status then reported attribution=adopted, born=cold, and a
changed session generation. Ordinary status/text remained readable and showed
the measurement's foreground still present. Decision: respect the ownership
refusal; do not reacquire it through a different shell, bypass the retained-block
boundary, signal descendants, start another measurement, or close the adopted
sessions. The auxiliary observation shell s-5a589d44a349fbc00824 was also left
untouched after this transition.

A later ordinary text read also became unavailable: fleet resolution reported
that m157's label binding was missing or conflicted. No alternate seat, address
or identity was guessed to get around that refusal.

Completion, generated reductions, standing tests, cleanup and log checksum are
unverified. The existing source gate and artifact smoke results remain valid;
rollout measurement and installed adoption remain deferred. No source change
is justified by a drive-ownership interruption. Future work should inspect the
supported session result only when its ownership permits, retaining this exact
run and avoiding duplicate proof worlds.
