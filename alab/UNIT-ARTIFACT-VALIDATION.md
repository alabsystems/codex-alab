<!-- Copyright 2026 Andrew Yates -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Cargo test artifact provenance

The native compiler shim places intermediates in `build.noindex`. Without an
explicit build directory, the preparation's `--no-run` and execution's additional
`--config` runner can select different layouts in nested Cargo. That rebuilds the
libraries elsewhere and makes the gate correctly refuse every unexpected artifact
before any test runs.

The unit-test phase now pins `CARGO_BUILD_BUILD_DIR` once, retaining an explicit
caller value or using `build.noindex` under its target. Metadata, compilation, and
execution share that layout. The original exact artifact-path, snapshot, package,
selector, process-context, and receipt checks remain unchanged.

The native Cargo fixture uses that layout too. It checks the full environment
against a snapshot taken immediately before the adapter in the same Cargo child,
rather than comparing transient delegation endpoints from different invocations.
Signal, descriptor, inherited-file-position, and competing-runner tests remain.
