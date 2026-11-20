# Official 0.170.0 release review

OpenAI published `rust-v0.170.0` on 2026-11-18. Its annotated tag `72f97c309f` points to `29eb79e986`. Four commits followed the shared branch point `72532ecae5` on the release line. All three behavioral changes have counterparts in official main and in the reviewed cumulative source `f7c191a8ff`. Keep tracking main. [Official release](https://github.com/openai/codex/releases/tag/rust-v0.170.0)

| Release commit | Official main | Private-main decision |
| --- | --- | --- |
| `2c4d576654` | `27e2227b2b` | Let the regular upstream merge bring the explicit Bedrock providers’ incremental-tools default and its opt-out. The patches share a stable patch ID. |
| `6c36e408a1` | `cb64621bf3` | Backport the Mantle commentary replay fix. Both versions have identical added and removed lines. Their patch IDs differ because the surrounding Core event handler changed. |
| `176cbbe92c` | `6c0cee502d` | Private main already carries the complete first-party catalog in `fdb80b2ec2`. Import the missing Bedrock and GCP adapters, their request assertions and picker expectations. |
| `29eb79e986` | Release packaging | Keep the fork’s own product version. The tag tip only changes the Rust workspace version. |

The release and official main have identical bundled model catalogs and provider adapters at their respective model commits. Main updated 61 additional test snapshots. On the shared paths, the prompt-prefix test supplies the same catalog through different test harnesses. No protocol schema change is required for these backports.

The official macOS ARM64 archive matched GitHub’s SHA-256 `7c9f21d5cbae4860d14b379fdcf709ff399ce874fd714b58783e1b4199c2b9a9`. Five loopback app-server sessions exercised `model/list`, `thread/start` and actual Responses requests with that executable and the cumulative development CLI. Both advertised the Sol and Astra 6.2 aliases. Default Mantle requests used `openai.gpt-6.2-sol`, default Runtime requests used `global.openai.gpt-6.2-sol`, and explicit US Runtime and Vertex selections used the requested model. Bedrock offered Astra's optional ultrafast tier. On tool continuation and the following turn, Mantle removed empty commentary annotations, preserved citations and retained output order. Runtime preserved the empty list.

The pre-backport private development binary still chose Bedrock 6.1 defaults and omitted 6.2 from the provider catalogs. It and signed ALab 0.11 both replayed the empty Mantle annotations. The loopback observations cover client traffic and use fixture credentials. The maintained regressions include `bedrock_function_continuation_replays_assistant_commentary`, `vertex_requests_use_command_auth_with_namespaces_and_tool_search` and `bedrock_model_list_advertises_ultrafast_without_changing_default`.

## Existing cloud sessions

Importing the official catalogs made 6.2 visible to new sessions. An existing
Mantle, Bedrock Runtime or Vertex thread still sent its next request to its saved
model. Development source `582805678a` follows a release in the saved family and
routing namespace when the built-in provider advertises it.

| Provider | Saved Sol → next request | Saved Astra → next request |
| --- | --- | --- |
| Bedrock Mantle | `openai.gpt-6.1-sol` → `openai.gpt-6.2-sol` | `openai.gpt-6-astra` → `openai.gpt-6.2-astra` |
| Bedrock Runtime | `global.openai.gpt-6.1-sol` → `global.openai.gpt-6.2-sol` | `us.openai.gpt-6-astra` → `us.openai.gpt-6.2-astra` |
| Vertex | `gpt-6.1-sol` → `gpt-6.2-sol` | `gpt-6-astra` → `gpt-6.2-astra` |

Thirty-two interactive and noninteractive resumes preserved the thread ID,
original prompt, assistant reply and saved effort. Fourteen threads were created
by signed ALab 0.11.0 with Ultra. Their resumed requests sent `xhigh` despite
a new global Low preference. An explicit US Runtime model pin kept 6.1.
An administrator-supplied catalog retained its saved model or selected its
declared replacement, including a replacement with an unrelated name.

Twenty additional cases exercised TLS routing with signed-build threads,
synthetic credentials and loopback endpoints. The commercial Mantle endpoint
received the 6.2 successor. GovCloud withheld commercial promotion and kept
the saved model. When only instance metadata supplied the region, browsing
did no metadata I/O and exposed 11 provisional rows. Thread admission resolved
the region and subsequent GovCloud browsing exposed two rows. The fixture
verified the AWS request signatures and regions on all ten SigV4 requests.
These observations cover client routing. AWS entitlements require service-side
evidence. The development executable's SHA-256 was
`ecafa2987eb61b55d0707a9e606a537bc92f0b464914022d39ada13748a6bb69`.

Some signed 0.11 rollouts stored only a bare AWS model name. That record gives
no evidence for choosing a global or US Runtime inference profile. The fork
preserves the saved selection in that case.

## Bedrock incremental tools on the merged source

Candidate `d3d2c9ee2b` includes official main through `a05444bbeb`. Its
Bedrock Mantle and Runtime requests matched the official 0.170.0 binary in
all six combinations of provider and configuration. With the setting omitted
or enabled, both clients sent the deferred tool definitions in an
`additional_tools` input item. With the setting disabled, they sent the
definitions in the top-level `tools` list. Private source `601bce9f`
required an explicit opt-in for the same behavior.

Each combination completed three requests through a loopback provider. The
comparison checked tool placement on all 54 requests across the three
executables. The upstream merge supplies the default and preserves the
opt-out without a separate ALab patch.

## Source of the subsequent prerelease

On November 19 the latest public prerelease was `rust-v0.172.0-alpha.1`.
Its annotated tag object `b14d60ead6` points to `b122134abd`.
The release commit changes one line in `codex-rs/Cargo.toml`, setting
the workspace version to `0.172.0-alpha.1`. Its parent `081f205f42`
is already in the reviewed official main at `83f73a818c`. Tracking that
main therefore includes the prerelease's functional source while preserving
the fork's product version.
[Official prerelease](https://github.com/openai/codex/releases/tag/rust-v0.172.0-alpha.1) · [Release commit](https://github.com/openai/codex/commit/b122134abda7e0e15ff24b087910f6aa53e700c2)
