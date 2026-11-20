<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Integrating upstream through d52a39b307

OpenAI’s change makes attached Guardian reviews honor elastic token budgets.
The ALab integration kept the upstream implementation. The intake check
accepted all 426 commits between private main’s upstream baseline and this
revision. [Upstream change](https://github.com/openai/codex/commit/d52a39b307)

## Model recovery

The 0.12 backport and upstream now have identical implementations of
`input_call_id`, `output_call_id` and `is_matching_tool_output`. The 0.13
integration uses the upstream functions. Keep the model-retirement observer
on the provider’s opened response before response interceptors run. This
lets it see a refusal from the model that served the request. The 0.12
insertion point belonged to an older sampling API. [Model recovery and runtime checks](../reviews/openai-0.170-alignment-plan-2026-11-18.md)

For `f863ae980a` against private main `7d27b649f7`, the added and removed
marker lines for all eight topics with lower counts matched the earlier
review of `ddda3567c5`, including the path and multiplicity of every line.
The native gate then accepted the committed intake and requested exact-source
evidence for 14 affected features. [Reviewed integration](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-f863ae980a)

## Queued input during a model refusal

The native build of `ddda3567c5` found that the fork’s review helper accepted
a slice of protocol inputs, while upstream Core passed resolved inputs.
The decision uses the slice to check whether Core accepted any input after
hooks ran. On both release lines the helper now takes the original slice of
either type. The accepted turn, input origin, queue selection and history
acknowledgement still determine when a finding can be cleared.
[Shared helper](https://github.com/alabsystems/codex-alab/commit/95ab71c8ff)

The next integration, `617fd59934`, includes the private changes for delayed
steering, saved input and resume through `578444e290`. Of the 37 changed
paths, 35 apply with identical added and removed lines. In the other two,
the helper accepts upstream’s generic turn request, and Core checks a strict
steer under the active-turn lock before acting on a sampling interrupt.
Upstream records realtime and dictation metadata before the fallback trigger
is normalized. The runtime checks cover a delayed steer after a missing
reply, bound thread ownership, and explicit queue corrections.
[Integrated source and checks](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-617fd59934)
