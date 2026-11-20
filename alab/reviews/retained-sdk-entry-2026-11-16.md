<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Retained SDK replacement transport and bound debug probe

The SDK now owns replacement preparation and entered-image observation on the same independent controller. It retains the actual pending operation, callbacks and captured data across canceled observations. A granted command requires both the correlated Native response and physical G1 offer; entered observation similarly requires E1. Offers require Native write EOF before Source acknowledges, reject ancillary rights and trailing bytes, and preserve uncertain outcomes. The executable command consumes its actual received recipe under the existing FD custody guard. These primitives require the paired Native half-close change and a real UI/input/resource consumer; ordinary activation remains disabled.

The explicit debug probe can construct the real BoundBackend before Source/config/auth, then use the complete existing in-process runtime and initial primary path. It uses an explicitly isolated fixture provider, the original environment/network-policy construction and the real capability reporter. Every post-bind exit drains the bound owner. Its private selection file contains only the actual plan claim. The paired fixture must obtain build IDs from its real store; this entry publishes no hardcoded IDs. No fabricated Native receipt, model turn or ordinary default is supplied.

Source-selected Trust 15698, with the existing protected profile, passed 316 distinct SDK tests (four existing child helpers ignored), compiled the explicit debug test executable, and linted both targets. The five new controls cover fragmented G1/E1 and EOF-before-ACK, partially polled offers, malformed/trailing bytes, actual ancillary-right adoption/closure, and closed command decoding. Final owned/spanning/failing/build-script lint findings are all zero; SDK has zero set-aside findings and the probe has two upstream findings. Neither selected package has a build script.

Final checked input is base `33f6040f1404bb7f14dc34d7704619169c3f3b7b`, patch SHA256 `a664c947ee96a3201f783441cf7e7a6b1ff8eddeefb037b28660058bec69aa01`, ten code/lock paths. The passing SDK input has identical SDK bytes. The normal resolver added only six existing dependencies to the probe's own lock entry; package versions/checksums and trust.lock are unchanged. Initial lint findings were fixed without allowances. Removing an identity map exposed a reference/value mismatch in the next compile; explicit Result::copied now passes compile and lint. Earlier inputs and failed logs remain outside the source tree.

These are unverified component executions and debug compilation, not a certified application build. Full original UI/editor/input transfer, selected-primary/NoPrimary construction ownership, an actual signed-image Native/Source handoff, certified application qualification and main landing remain required before the live-update goal is complete.

## November 18 two-image retained work

A disposable, SIP-enabled macOS 27.2 arm64 guest (26B123) ran the original SDK
from private main `a16eb8d24d684998becec48d896519b3cb9da77c` and the replacement
from the 408-change cumulative source `8c12251a4e56252fae915eb951540e84ea0612cf`.
Each source added the same fork-owned test files. The four inputs below retained
their SHA-256 values before and after execution.

| Input | Tested source | Executable SHA-256 |
| --- | --- | --- |
| Original SDK | `b2981c018af237abade44e28e05a478f1a2f8567` | `1da3ded87b223f70bbf5c94314574fda58d480945447486227be012efb129363` |
| Replacement SDK | `860996be667aa24eff5961a590dbd01b0dc5ab39` | `9e21b9b98757dff4acab7a801b52833c6c38d1737dbdb858c02156b0a0f209ea` |
| Native host test executable | Released aterm 0.207.0 source `ef072bbe1cfd9b8f030985b6ff8824a45834684a` | `e9d56280c1155e28f5e962c696b1e678f3c82cc8befdb38cea38ceb582817a4d` |
| Native guest driver | Existing `aterm-lab mac codex-source-proof` | `c0c43fc5a36e5358833a1a6bcc9a46239d5209be2683ee878681d7ec1cfe0612` |

Both SDKs used the repository's authenticated, source-selected Trust 15698 and
protected test profile. The host fixture was built separately with its selected
Trust 27009. With the two qualification packages, host executable and driver
identified above, the producer and native driver commands were:

```sh
targo --unverified test --locked --offline -p codex-alab-native-source-probe \
  --features native-source-proof --test native-source-proof --no-run \
  --message-format=json-render-diagnostics
guest=ghcr.io/cirruslabs/macos-golden-gate-base@sha256:4859ea9483a1f6e377301f23a5243798044266a373fc36635e9dcd045e09292f
"$lab" mac codex-source-proof --image "$guest" --kind retained --aterm "$host" \
  --package "$original_package" --replacement-package "$replacement_package" --record "$record"
```

The original Core accepted a real `thread/shellCommand` and the shell wrote its
first marker. The foreground process then exec'd the replacement. The replacement
checked its original PID and its new executable device and inode, reattached to
the original thread through the real native controller, and started its own
independently admitted backend. Each backend's `thread/loaded/list` excluded
the other's thread. The replacement completed a new shell turn before it
released the original command. The original completion matched its thread,
turn, item, output and exit code 0. Its workspace contained one start and one
completion. After the frontend exited, native code observed exactly the two
expected SDK backend images by their authenticated leases. The component and
driver exited 0 with complete capture. The observed operation was unfinished
shell work when the image changed. Replay of an already completed RPC awaiting
acknowledgment remains a separate case.

The native receipt `current-pair-guest-observation.json` has SHA-256
`65af895beed97455e5311b12feb445c50b6d9b69e3f24f777cf7b7d69aad7f3b`.
The dedicated executables used the debug-only Source credential ABI and the
isolated candidate marker. Both reported exec custody 0 and no runtime DATA
readers. An independent physical image and `debug upgrade-capabilities` check
found the following ordinary client behavior.

| Ordinary executable | Runtime DATA writer | Runtime DATA readers | Exec custody | Session Source credential ABI |
| --- | --- | --- | --- | --- |
| Signed 0.11 build 2026111701, `70d6391259` | 2 | 2 | 2 | Absent |
| 0.12 development CLIs `8c12251a4e` and `fbc7e3c982` | 4 | 2, 3, 4, 5 | 2 | Absent |

The five-image observation `ordinary-and-test-image-abi.json` has SHA-256
`43f9f5041782e6f85cffbaa291ec840fd1b2f017c4ae68d326271a18cca14e53`.
On the installed, signed aterm 0.207.0, `aterm ctl @self native-act info
source-exec` returned `preparation=1 execution=unavailable input-fence=1`.
The normal updater checks this reply before reserving the source. The host also
requires an exact registered original actor, signed package leases and a fresh
destination credential constructor. The completed test changes neither the
installed Codex 0.11 package nor that host admission. A normal signed upgrade
still requires the managed updater to preserve the original terminal, editor
and runtime through settlement and adoption. [Signed DATA and credential checks](../design/goal-upgrade-admission.md)
