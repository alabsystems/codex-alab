<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Bound the aggregate control-socket Lines reply

Decision: cap each Lines reply at 256 MiB of body wire bytes, matching the native
`aterm-ctl` collector. A line count and an independent line limit are insufficient:
the previous reader could retain 200,000 lines of up to 8 MiB each. A fast local
peer can allocate substantial memory before the existing exchange deadline ends.
The native counterpart was inspected at aterm source
`518ad6b06504308887befb6edca8a741efa5281f`.

The reader now charges the original bytes, including LF/CRLF, against a per-reply
budget. It restricts each read to remaining room before allocating the line.
The existing line/count limits, lossy UTF-8 text decoding, status/Bytes framing
and exchange deadlines retain their behavior. An over-budget reply is Malformed;
the existing request worker closes on any parser error rather than reusing the
partly consumed stream. This is a wire-byte bound, not a measured RSS limit:
replacement characters and allocation capacity add bounded text overhead.

Four new regression methods use the production reader with a small injected
aggregate budget. They cover exact limits, UTF-8 byte counts, malformed UTF-8
replacement, CRLF and empty-line accounting, preserved next-reply framing,
read truncation before the remaining fixture tail, EOF, empty/error replies and
independence of status/Bytes framing. No 256 MiB fixture allocation or process
memory measurement is needed or claimed.

On source base `3ffd1bcadc` plus this change, direct source-selected Trust 15698
(`trustc 0.11.0+g20565c325`, compiler commit
`20565c3255adf151f9eb920e89cdf83c38f84abe`) runs the complete
`codex-alab-aterm --lib` suite in the explicit unverified test lane: 239 passed,
zero failed, three existing ignored subprocess entrypoints, zero filtered.
Scoped `targo tippy --locked -p codex-alab-aterm --lib --tests --no-deps -- -D warnings`
also exits zero, using a separate target. These ordinary library/lint observations
are not the native gate, an application certificate or package qualification.

Final implementation SHA-256:
`96e6294c0dbecbe93163312a0ecf159e961606dc8fd8695da0d533d934bf6c2d` (`conn.rs`),
`c3fec0f520a831f7055d40d4aacb6ee22761120d45f3c9f7374d76a24ca0aeb4` (`conn_tests.rs`).
The isolated worktree's `codex-rs/target/aterm-lib-final-15698.log` has SHA-256
`ebcba016b6437bdee062628d02e15db9aec378a1c71d680efdbf106850b60da5`;
`aterm-tippy-final-15698.log` has SHA-256
`435fe04f0df5a40b77b93895f3cae5d12af45499808b9016e4723df85b1d9a10`.
