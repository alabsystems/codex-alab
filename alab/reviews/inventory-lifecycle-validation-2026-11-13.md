<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Reject malformed feature lifecycles with a validation result

After the recovery work, the live feature inventory reported complete
ownership for all 81 hook topics. The earlier note about five unowned topics
no longer describes current main. GitHub issue discovery was unavailable:
the dev identity returned a non-retryable HTTP 403 requiring `issues=read`.
No alternate identity or permission workaround was attempted; continue with
local source review.

That review found that a TOML array or table in a feature's `lifecycle` caused
an uncaught `TypeError` during set membership, rather than the existing
`invalid lifecycle` validation result. Both paths reject the catalog, but
the exception omits the feature-level diagnosis and interrupts aggregation.

Require a string before checking the three named lifecycle values. Tests
exercise malformed values directly and through committed TOML catalogs;
the CLI must exit one, report the invalid feature, emit no traceback, and
never report complete ownership. Before the change the regression produced
four unit errors and two CLI assertion failures for arrays and tables.
This changes no valid catalog, topic ownership rule, or qualification claim.

After the fix all 27 inventory tests passed. The broader feature-family run
exposed a separate selected-profile fixture missing `trust_memory.py`; that
failure does not involve lifecycle validation and is handled separately.
