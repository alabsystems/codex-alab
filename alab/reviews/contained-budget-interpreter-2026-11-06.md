# Containment-budget fixture interpreter

The next containment companion still assumed Apple's /usr/bin/python3 was
available, both for account-home discovery and its sandboxed probes. On this
Mac the baseline absent-limits method failed before the boundary: the launcher
requested developer tools and the account-home lookup failed.

Decision: bind ALAB_PYTHON to sys.executable in the fixture environments and use
that same interpreter for their child probes. The physical-cache serialization
fixture also binds its guardian interpreter. Caller limit values and explicit
Trust probes remain as before; no lib.sh, profile, resource limit or production
permission changed. This exercises the managed runtime the actual package uses
without depending on Apple's optional tool launcher.

All eight methods passed in 10.503 seconds, with no skips. Actual Seatbelt checks
cover admitted/absent compile-test limits and unrelated environment scrubbing;
read-only unprivileged process inspection; named Trust sysroot reads with writes
refused; rejection of a sysroot at or above a home; and private HOME isolation
from owned account canaries, preferences services and owned CFPreferences-named
shared-memory objects. Only newly created exclusive fixtures are touched, and
the test does not contact preference services. Explicit owned reads and ordinary
owned shared memory remain usable.

The physical-cache test also completed: aliases contend for one kernel lock,
another physical cache proceeds independently, and release permits a successor.
Children reach EOF before fixture removal. Diff whitespace checks passed. No
native build ran and no installed package was changed. The separate remote
rollout measurement remains deferred, without a duplicate run.
