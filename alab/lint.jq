# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/lint.jq: which of tippy's diagnostics are ALab's (alab/check.sh, step 4). Run it as
#   jq -R -n -f alab/lint.jq --rawfile hooks <hook-lines> --rawfile pristine <json-or-empty>
#     --argjson roots <json> --argjson proots <json> --arg ws codex-rs/ --argjson compared <bool>
#     --argjson scripted <json>
# with cargo's --message-format=json lines from the ALab tree on stdin (a line that is not
# JSON is skipped). <hook-lines> is `git grep -n --full-name`'s "<path>:<line>:..." for each
# upstream line that carries an ALAB marker. <pristine> holds the same JSON lines from a run
# on pristine upstream, and <compared> says whether one ran. <roots> and <proots> are the
# spellings of each tree's root directory, each ending in /, and <ws> is the cargo
# workspace's directory in the repo. <scripted> lists the manifests (as cargo metadata spells
# them in the ALab tree) of the linted packages that have a build script: ALab's crates and
# the hooked upstream ones. Paths are compared relative to the repo root: cargo
# names a file in the workspace relative to it (glue an upstream crate compiles by #[path]
# as tui/src/../../alab-tui/glue/x.rs, so ../ is resolved), and one outside it absolutely
# (alab/vendor/allocative, a [patch] path).
#
# Each warning or error is located at its primary spans and at the call site of each macro
# expansion they come from. It prints one JSON object:
#   owned     the rendered text of each distinct diagnostic located in ALab's code: a file
#             under <ws>alab-* or alab/, or a span that starts on a hook line
#   spanning  each distinct other diagnostic with a span that covers a hook line it does not
#             start on (a method chain a hook joins midway is reported from its receiver):
#             {rendered, manifest, ours}, where ours (null until <compared>) says the ALab
#             tree has more distinct diagnostics of its kind (level, code, message, files) than
#             pristine upstream, so its hook brought one in
#   failing   each target with an error: {manifest, target, errors, upstream_too}, where
#             upstream_too (null until <compared>) says pristine upstream's run has an error
#             in that target too
#   scripts   each <scripted> package whose build script did not run to completion in the
#             ALab tree, and whose build script has no compile error of its own (that one is a
#             failing target): {manifest, built, upstream_too}. Cargo's JSON records a build
#             script that ran (build-script-executed, fresh or not) and none that failed, which
#             it says only in prose; with the script gone, no other target of the package is
#             built, so no diagnostic of it says anything. built says the script compiled (its
#             custom-build artifact) and so failed when it ran; otherwise it never compiled,
#             because something it needs failed. upstream_too (null until <compared>) says
#             pristine upstream's run compiled that package's build script and it did not run
#             there either: only that evidence counts, so a package pristine did not lint gates
#   set_aside how many distinct located diagnostics are upstream's own: neither owned nor
#             spanning, or spanning and not ours (a summary such as "2 warnings emitted"
#             names no file and is not counted)

def resolved: reduce (split("/")[] | select(. != "" and . != ".")) as $p ([];
  if $p == ".." and length > 0 and .[-1] != ".." then .[:-1] else . + [$p] end) | join("/");

# in_repo(roots): a path cargo printed, relative to the repo root. One outside the repo keeps
# its leading / and so is never ALab's.
def in_repo($roots):
  . as $f
  | if startswith("/") then
      ([$roots[] | select(. as $r | $f | startswith($r))] | first) as $r
      | if $r == null then "/" + resolved else $f | ltrimstr($r) | resolved end
    else $ws + . | resolved end;

def sites: ., (.expansion.span // empty | sites);

# records: the JSON objects among a list of cargo's raw lines (a line that is not one is skipped).
def records: [.[] | fromjson? | select(type == "object")];

# diagnostics(roots): each warning and error among cargo's records.
def diagnostics($roots):
  [.[] | select(.reason == "compiler-message")
    | {manifest: (.manifest_path | in_repo($roots)),
       target: (.target | {kind: (.kind | join(",")), name})} as $from
    | .message
    | select(.level == "warning" or (.level | startswith("error")))
    | $from + {level, message, code: .code.code, rendered,
        primary: [.spans[] | select(.is_primary)
          | {file: (.file_name | in_repo($roots)), line_start, column_start}],
        sites: [.spans[] | select(.is_primary) | sites
          | {file: (.file_name | in_repo($roots)), line_start, line_end}]}];

def place: [.level, .code, .message, .primary];
def kind: {level, code, message, files: ([.primary[].file] | unique)};
# counted(diagnostics): how many distinct places each kind of diagnostic is at.
def counted($ds): $ds | unique_by(place) | group_by(kind) | map({key: (.[0] | kind | tojson),
  value: length}) | from_entries;

# build_scripts(roots): among cargo's records, the manifests whose build script compiled
# (built) and those whose build script then ran (ran). Cargo names the package by its id in
# build-script-executed and gives the manifest only with the artifact, which comes first.
def build_scripts($roots):
  ([.[] | select(.reason == "build-script-executed") | .package_id]) as $ran
  | [.[] | select(.reason == "compiler-artifact" and (.target.kind | index("custom-build")))]
  | {built: (map(.manifest_path | in_repo($roots)) | unique),
     ran: (map(select(.package_id | IN($ran[])) | .manifest_path | in_repo($roots)) | unique)};

($hooks | split("\n") | map(select(. != "") | split(":") | {file: .[0], line: (.[1] | tonumber)})
  | group_by(.file) | map({key: .[0].file, value: map(.line)}) | from_entries) as $hook
| def alab_file: test("^(" + $ws + "alab-[^/]+|alab)/");
  def starts_on_hook: . as $s | $hook[$s.file] // [] | any(. == $s.line_start);
  def covers_hook: . as $s | $hook[$s.file] // [] | any(. >= $s.line_start and . <= $s.line_end);
([inputs] | records) as $ours_records
| ($pristine | split("\n") | records) as $theirs_records
| ($ours_records | diagnostics($roots)) as $ours
| ($theirs_records | diagnostics($proots)) as $theirs
| ($ours_records | build_scripts($roots)) as $scripts_ours
| ($theirs_records | build_scripts($proots)) as $scripts_theirs
| counted($ours) as $n_ours | counted($theirs) as $n_theirs
| [$ours[] | . + {own: any(.sites[]; (.file | alab_file) or starts_on_hook)}
    | . + {spans: ((.own | not) and any(.sites[]; covers_hook))}
    | . + {ours: (if .spans and $compared then $n_ours[kind | tojson] > ($n_theirs[kind | tojson] // 0)
        else null end)}] as $all
| {owned: [$all[] | select(.own)] | unique_by(place) | map(.rendered),
   spanning: [$all[] | select(.spans)] | unique_by(place) | map({rendered, manifest, ours}),
   failing: [$all[] | select(.level | startswith("error"))] | group_by([.manifest, .target])
     | map({manifest: .[0].manifest, target: .[0].target,
         errors: (unique_by(place) | map(.rendered)),
         upstream_too: (if $compared then (.[0] as $t | any($theirs[];
           (.level | startswith("error")) and .manifest == $t.manifest and .target == $t.target))
           else null end)}),
   scripts: [$scripted | map(in_repo($roots)) | unique[] as $m
     | select(($m | IN($scripts_ours.ran[])) | not)
     | select(any($all[]; (.level | startswith("error")) and .manifest == $m
         and .target.kind == "custom-build") | not)
     | {manifest: $m, built: ($m | IN($scripts_ours.built[])),
        upstream_too: (if $compared then ($m | IN($scripts_theirs.built[]))
          and (($m | IN($scripts_theirs.ran[])) | not) else null end)}],
   set_aside: [$all[] | select((.primary | length > 0) and (.own | not)
       and ((.spans | not) or .ours == false))] | unique_by(place) | length}
