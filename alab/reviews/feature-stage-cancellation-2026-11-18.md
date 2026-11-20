# Feature-stage cancellation and workspace custody

Candidate `96042e0c18` was executing the qualification fixture's native TUI
stage when its outer recipe received TERM. The campaign recorded exit 143
and released the worktree. Its log SHA-256 at that point was
`c369cb1d7658d8daf1a23b7abc65b2b442c142005f2a55c5872ba0b93e631384`.
The selected controller and the protected compiler continued. The final log
digest was `84a67649557836538a3c801ef05ca857abfd6554cc8d2a168288c4b86f9e362f`.
The existing digest check rejects that receipt. The original nested guardian
subsequently retired its compiler before the workspace advanced.

The outer Bash recipe had launched the native publisher as a child. After
the shell exited, the campaign custodian stopped its command group. The
publisher's selected controller ran in a separate native cohort. The publisher
is the component that forwards cancellation to that cohort's original command,
waits for it to drain and rechecks the source and compiler.

The outer recipe now becomes the authenticated publisher through `exec`.
The campaign custodian sends TERM to this original, unreaped child and waits
for it while holding the workspace lock. Archive the earlier failed attempt
and start fresh after the owner settles. The existing native selection and
process authority cover this change.

[Campaign runner](../feature_campaign.py) · [Custodian](../command_guard.py) · [Native cancellation regression](../tests/test_feature_campaign_cancellation.py)

The protected macOS probe used the native publisher from source
`6b9844f57cf` and a short controller with a held final log write. TERM sent
to the original stage and TERM sent to the collector both reached the native
controller. The corresponding caller stayed alive until the write completed.
After an uncatchable signal to the collector, the surviving custodian retained
the worktree lock through the same write and native settlement. The regression
also compared the outer launch PID with the publisher's PID. Both tests passed
on topic `b48a8da29f`. Another 119 guardian, project-selection and campaign
tests passed.
