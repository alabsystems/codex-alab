# Bind local publication to the inspected commit

The publication helper captures `HEAD^{commit}` and checks that it descends
from expected main. Its Agh path already sends that exact object ID, but its
explicit local-repository path previously pushed `HEAD` again. A checkout or
commit between validation and push could therefore publish a different,
uninspected commit. Push the captured ID on that path as well. Keep ordinary
fast-forward enforcement, source guards and Agh's network ownership intact.

The regression changes HEAD immediately before the real local push. It fails
against the old implementation because the remote receives the later commit;
with the fix, the remote receives only the inspected candidate. The local path
invokes no native network client. All 24 GitHub transport/publication tests
and five sync push-race tests pass; whitespace checks pass too.

This review began after a successful native publication was followed by a
redundant fetch that hit Agh's background-read quota reserve. The native
acknowledgment is evidence that the exact submitted update succeeded at that
point; it does not establish that main has not advanced since. Retain the
submitted before/after IDs with that result. A refused follow-up fetch leaves
tracking refs stale, not the already acknowledged publication unperformed.
Respect the recorded quota reset instead of retrying through another identity
or transport. This operational observation does not change the helper's output
format or its refusal behavior.


## Complete caller verification

On clean source `86fec085a1d99ad92a815c2479df2d98c8c5a555`, the complete
Python tooling suite passes: 956 tests, zero failures, one skipped subcase,
in 1073.508 seconds. All seven native package/cache/controller opt-ins were
enabled with the retained development tools. The sole skip is the non-UTF8
filename this filesystem cannot represent. aterm reports no check process
left running after completion. The log and manifest are retained under that
source's local validation directory at `tooling-publication-full/`.

After the recorded quota reset, one authoritative main fetch succeeds and
shows no concurrent main changes. This permits publishing the reviewed fix
without repeating the refused read or changing credentials. The tooling
result is separate from the still-unavailable certified application build.
