<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Record the current selected-profile boundary in its fixture

The feature-family check exposed three failures in the selected-profile
argument test. The unchanged `2c2df12a42` source reproduced all three, failing
before recording any arguments because the scratch fixture has no
`alab/trust_memory.py`.

The intended test calls real `check_selected` and records its containment
arguments, without invoking or claiming native execution authority. Its
override targeted `alab_contained`, but production now calls
`alab_verified_contained`, which runs the memory wrapper and starts a fresh
shell. The inner function override therefore cannot reach the child.

Record the arguments at `alab_verified_contained` instead. This restores the
fixture's original boundary, tests missing and existing evidence bundles and
spaced paths, and leaves the production memory and containment code unchanged.
Do not copy the missing helper into the fixture and accidentally turn an
argument test into a real native-containment execution.

Validation after the repair: all 61 feature-family tests and all 16 topic
inventory tests passed with the managed Python interpreter. The lifecycle
validator regression is a separate commit; no Rust or production containment
code changed in this fixture repair.
