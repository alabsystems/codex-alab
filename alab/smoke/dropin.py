#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Is codex-alab still a drop-in `codex`? A packaged codex-alab against the vendor's codex,
both run with a scratch HOME, CODEX_HOME and XDG_RUNTIME_DIR and no aterm in the
environment (so neither reaches a real aterm session or the ~/.codex that holds a login).

Checks, each on both binaries:

  name      the package's entrypoint is bin/codex, and `--version` says `codex-cli <v>`.
  help      the subcommand and flag tree, from `--help` recursively: every page's
            subcommands, its flags (long and short, whether each takes a value, and its
            possible values) and its positional arguments as written. Every difference is
            attributed from the source: an item pristine upstream has at the commit
            recorded for the fork source (the official tag on a release leaf) is upstream's move
            since the vendor's version; ALab's own crates and its marked clap command
            declarations establish its additions; anything else fails. A short flag, a
            value or an argument the vendor has and
            codex-alab lacks fails unless upstream's source no longer names it; with no
            upstream/main to read (a clone alab/setup.sh has not set up) the check fails.
  exec      `codex exec --experimental-json` (the TypeScript SDK's argv) runs a turn against
            a local mock Responses server: exit 0, the mock's reply in the JSONL, a
            turn.completed event; and the events' fields (every key path, per event and item
            type) are the vendor's, attributed as above.
  app-server `codex app-server --listen stdio://` (the Python SDK's argv) answers initialize,
            and `app-server generate-json-schema --experimental` (what the Python SDK is
            generated from) names the vendor's definitions, properties and enum values; a
            difference is compared at its file and owner against the vendor tag, upstream merge
            base and ALab source exports. ALab additions require a declared payload or hook.
  config    the effective config (`config/read` over the app-server), outside aterm and
            with TERM_PROGRAM=aterm: codex-alab with no config.toml reads what the vendor
            reads with ALab's packaged defaults (alab-core/defaults.toml, and
            aterm-defaults.toml over it inside aterm only, at the package's source commit) as
            its config.toml, but for upstream's own moves since the vendor's release tag.
            ALab's own keys (named `alab_*`, such as `tui.alab_progress`) are no field of
            upstream's typed config: config/read shows them in neither build.
            When the package declares a reasoning effort, an ephemeral thread started
            without a model or effort override must return that effort.
            Each ALab feature default flips back through config.toml, `-c features.<f>=true`
            and `--enable <f>`.
  mcp       an MCP server a config.toml declares is what `codex mcp list --json` lists.
  home      compare files left in CODEX_HOME by shape, and upstream-added bundled skills
            by exact path and bytes against both source revisions. `codex login
            --with-api-key` (file store) writes auth.json, and `login status` reads it.

usage: dropin.py [--alab <codex-alab package dir>] [--vendor <vendor package dir>] [--keep]
Defaults: the newest package under ~/.local/share/aterm/dev-builds/artifacts/codex (what
alab/package.sh writes) and the newest vendor build in atpkg's store (`codex-old`, or `codex`
before the rename). Exit 0 when every check passes, 1 otherwise. Runs only those two binaries;
builds nothing.
"""

import argparse
from contextlib import ExitStack
from functools import lru_cache
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from upstream_intake import integrated_upstream  # noqa: E402
from dropin_schema import archive_items, schema_items  # noqa: E402
from native import Mock, register_directory_cleanup, serve  # noqa: E402

REPO = subprocess.run(
    ["git", "-C", os.path.dirname(os.path.abspath(__file__)), "rev-parse", "--show-toplevel"],
    capture_output=True, text=True, check=True,
).stdout.strip()
ARTIFACTS = os.path.expanduser("~/.local/share/aterm/dev-builds/artifacts/codex")
# atpkg's stores of OpenAI's own build: `codex-old` since `codex` became codex-alab, `codex`
# before that.
STORES = [os.path.expanduser("~/Library/Application Support/aterm/pkg/store/" + name)
          for name in ("codex-old", "codex")]
PROMPT = "reply with ok"


def newest_alab():
    dirs = [os.path.join(ARTIFACTS, d) for d in os.listdir(ARTIFACTS)
            if not d.startswith(".") and os.path.isfile(os.path.join(ARTIFACTS, d, "alab-build.json"))]
    if not dirs:
        sys.exit(f"dropin: no codex-alab package under {ARTIFACTS}: run alab/package.sh")
    return max(dirs, key=os.path.getmtime)


def newest_vendor():
    builds = [(int(d), os.path.join(store, d)) for store in STORES if os.path.isdir(store)
              for d in os.listdir(store) if d.isdigit()
              and os.path.isfile(os.path.join(store, d, "codex-package.json"))
              and not os.path.exists(os.path.join(store, d, "alab-build.json"))]
    if not builds:
        sys.exit("dropin: no vendor codex in %s: aterm pkg install codex-old" % " or ".join(STORES))
    return max(builds)[1]


class Bin:
    def __init__(self, label, pkg, scratch):
        self.label, self.pkg = label, pkg
        manifest = json.loads(Path(pkg, "codex-package.json").read_bytes())
        self.entry = manifest["entrypoint"]
        self.version = manifest["version"]
        self.exe = os.path.join(pkg, self.entry)
        self.root = os.path.join(scratch, label)
        os.makedirs(self.root)
        self.n = 0

    def env(self, codex_home):
        home = os.path.join(self.root, "home")
        xdg = os.path.join(self.root, "xdg")
        for d in (home, xdg, codex_home):
            os.makedirs(d, exist_ok=True)
        # No TERM_PROGRAM, no ATERM_*: neither binary takes its aterm paths, and the
        # control socket it would look for lives under this scratch XDG_RUNTIME_DIR.
        return {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": home, "XDG_RUNTIME_DIR": xdg,
                "CODEX_HOME": codex_home, "TERM": "dumb", "NO_COLOR": "1",
                "LANG": "en_US.UTF-8", "TMPDIR": os.path.join(self.root, "home")}

    def fresh_home(self):
        self.n += 1
        return os.path.join(self.root, "codex-home-%d" % self.n)

    def run(self, args, codex_home=None, stdin=None, timeout=60, cwd=None, extra_env=None):
        codex_home = codex_home or self.fresh_home()
        # stdin is closed unless given: `codex exec` reads a piped stdin into the prompt, and
        # an inherited pipe nobody closes would hold it there.
        return subprocess.run([self.exe] + args, env={**self.env(codex_home), **(extra_env or {})},
                              input=stdin,
                              stdin=None if stdin is not None else subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=timeout,
                              cwd=cwd or self.root)


# ---------------------------------------------------------------- help tree

SECTION = re.compile(r"^([A-Z][A-Za-z ]*):\s*$")
COMMAND = re.compile(r"^  ([a-z][a-z0-9-]*)(\s{2,}|$)")
# An option's own line is indented 2 (`  -m, --model <MODEL>`) or 6 (`      --oss`); its
# description is indented 10, so a `--flag` quoted there is never read as one.
OPTION = re.compile(r"^ {2,6}(?:(-[A-Za-z0-9]),\s+)?(--[A-Za-z0-9][A-Za-z0-9-]*)(.*)$"
                    r"|^ {2,6}(-[A-Za-z0-9])(?![A-Za-z0-9-])(.*)$")
ARGUMENT = re.compile(r"^  ([\[<][A-Za-z0-9_-]+[\]>](?:\.\.\.)?)(\s{2,}|$)")
INLINE_VALUES = re.compile(r"\[possible values: ([^\]]*)\]")
LISTED_VALUE = re.compile(r"^ {8,}- ([A-Za-z0-9][A-Za-z0-9_.-]*)(?::|\s*$)")


class Opt:
    """One flag on one help page: its short form, whether it takes a value (and how many),
    and its possible values (None when clap lists none)."""

    def __init__(self, short, rest):
        self.short = short
        rest = rest.strip()
        self.arity = "values" if re.match(r"^[\[<=].*\.\.\.", rest) else \
            "value" if re.match(r"^[\[<=]", rest) else "flag"
        self.values = None

    def add_values(self, values):
        self.values = (self.values or set()) | set(v for v in values if v)


def parse_help(text):
    """A help page as (subcommands, {flag: Opt}, [positional arguments as written]). A flag is
    keyed by its long form, or by its short form when it has no long one."""
    commands, options, arguments, section, current = [], {}, [], None, None
    listing = False
    for line in text.splitlines():
        m = SECTION.match(line)
        if m:
            section, current, listing = m.group(1), None, False
            continue
        if section == "Commands":
            c = COMMAND.match(line)
            if c and c.group(1) != "help":
                commands.append(c.group(1))
        elif section == "Arguments":
            a = ARGUMENT.match(line)
            if a:
                arguments.append(a.group(1))
        elif section == "Options":
            o = OPTION.match(line)
            if o:
                if o.group(2):
                    current = options[o.group(2)] = Opt(o.group(1), o.group(3))
                else:
                    current = options[o.group(4)] = Opt(None, o.group(5))
                listing = False
                current_values = INLINE_VALUES.search(line)
                if current_values:
                    current.add_values(v.strip() for v in current_values.group(1).split(","))
                continue
            if current is None:
                continue
            if line.strip() == "Possible values:":
                listing = True
                continue
            inline = INLINE_VALUES.search(line)
            if inline:
                current.add_values(v.strip() for v in inline.group(1).split(","))
                continue
            if listing:
                v = LISTED_VALUE.match(line)
                if v:
                    current.add_values([v.group(1)])
                elif line.strip() and not line.startswith(" " * 12):
                    listing = False
    return commands, options, arguments


def help_tree(b):
    tree, todo = {}, [()]
    while todo:
        path = todo.pop()
        r = b.run(list(path) + ["--help"], timeout=30)
        if r.returncode != 0:
            tree[path] = ("ERROR", r.stderr.strip().splitlines()[-1:] or ["exit %d" % r.returncode])
            continue
        commands, options, arguments = parse_help(r.stdout)
        tree[path] = (set(commands), options, arguments)
        if len(path) < 3:
            todo.extend(path + (c,) for c in commands)
    return tree


def upstream_base(source):
    """Resolve the package source against its committed release tag or upstream/main."""
    try:
        return integrated_upstream(REPO, source)
    except subprocess.CalledProcessError:
        return None


def checked_upstream_base(source, manifest):
    base = upstream_base(source)
    stamped = manifest.get("upstream")
    if stamped is not None:
        commit = stamped.get("commit") if isinstance(stamped, dict) else None
        if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
            raise ValueError("invalid upstream commit in alab-build.json")
        if base != commit:
            raise ValueError(f"package records upstream {commit}, but the source resolves to "
                             f"{base or 'unavailable'}; fetch the official upstream refs and verify the package source")
    return base


def grep(commit, patterns, paths):
    args = ["git", "-C", REPO, "grep", "-q", "-F"]
    for p in patterns:
        args += ["-e", p]
    return subprocess.run(args + [commit, "--"] + paths, capture_output=True).returncode == 0


def snake(word):
    return re.sub(r"(?<=[a-z0-9])([A-Z])", r"_\1", word).lower().replace("-", "_")


def named(commit, item, paths):
    """Whether the source at `commit` names `item` (a subcommand, a long flag, a value, an
    argument or a field), in `paths`. A one-letter item is a short flag: clap spells it
    `short = 'x'` (derive) or `short('x')` (builder)."""
    if commit is None:
        return False
    word = item.strip("[]<>.").lstrip("-")
    if len(word) == 1:
        return grep(commit, ["short = '%s'" % word, "short('%s')" % word,
                             "short_alias = '%s'" % word], paths)
    if not word:
        return False
    snake_word = snake(word)
    camel = "".join(p.capitalize() for p in re.split(r"[-_]", word))
    pats = ['"%s"' % word, "--%s" % word, "fn %s" % snake_word, " %s:" % snake_word, camel + " {",
            camel + ",", camel + "(", "%s =" % snake_word, '"%s"' % snake_word]
    # Experimental Rust types may be absent from committed schemas; keep their internal capitals.
    pats += [word + " {", word + ",", word + "("]
    return grep(commit, pats, paths)


def rust_declarations(text):
    """Mask non-code while keeping offsets and real line comments (the ALAB markers)."""
    token = re.compile(r"//[^\n]*|/\*|(?:br|r)(?P<hashes>#*)\"|b?\"|b?'(?:\\.|[^'\\\n])'")
    masked, cursor = list(text), 0
    while match := token.search(text, cursor):
        start, end = match.span()
        if match[0].startswith("//"):
            cursor = end
            continue
        if match[0] == "/*":
            depth = 1
            for boundary in re.finditer(r"/\*|\*/", text[end:]):
                depth += 1 if boundary[0] == "/*" else -1
                if depth == 0:
                    end += boundary.end()
                    break
            else:
                end = len(text)
        elif match["hashes"] is not None:
            delimiter = '"' + match["hashes"]
            closing = text.find(delimiter, end)
            end = len(text) if closing < 0 else closing + len(delimiter)
        elif match[0].endswith('"'):
            while end < len(text):
                char = text[end]
                end += 2 if char == "\\" else 1
                if char == '"':
                    break
        masked[start:end] = ["\n" if char == "\n" else " " for char in text[start:end]]
        cursor = end
    return "".join(masked)


def clap_subcommands(text):
    """Clap enum declarations, excluding similarly named data variants and handler arms.
    Unrecognized declarations provide no attribution evidence.
    """
    header = re.compile(
        r"(?m)^(?P<indent>[ \t]*)#\[derive\((?P<derive>[^\]]*)\)\]"
        r"(?P<attrs>(?:\s*#\[[^\]]*\])*)\s*"
        r"(?:pub(?:\([^)]*\))?\s+)?enum\s+(?P<enum>\w+)\s*\{")
    code = rust_declarations(text)
    for enum in header.finditer(code):
        if not re.search(r"(?:^|,)\s*(?:clap::)?Subcommand\s*(?:,|$)", enum["derive"]):
            continue
        if re.search(r"\brename_all\s*=", enum["attrs"]):
            continue
        rest = code[enum.end():]
        end = re.search(r"(?m)^" + re.escape(enum["indent"]) + r"\}", rest)
        if end is None:
            continue
        body = rest[:end.start()]
        variants = list(re.finditer(
            r"(?m)^(?P<indent>[ \t]+)(?:#\[cfg\([^\]\n]*\)\][ \t]*)*"
            r"(?P<variant>\w+)[ \t]*[,({][^\n]*", body))
        depth = min((len(v["indent"]) for v in variants), default=0)
        previous = 0
        for variant in variants:
            if len(variant["indent"]) != depth:
                continue
            attrs = re.sub(r"//[^\n]*", lambda m: " " * len(m[0]),
                           body[previous:variant.start()])
            original_attrs = text[enum.end() + previous:enum.end() + variant.start()]
            previous = variant.end()
            if re.search(r"#\[(?:command|clap)\([^\]]*\b(?:flatten|skip|external_subcommand)\b", attrs):
                continue
            explicit = []
            for attribute in re.finditer(r"#\[command\(\s*name\s*=[^\]]*\)\]", attrs):
                literal = re.fullmatch(r'#\[command\(\s*name\s*=\s*"([^"\\]+)"\s*\)\]',
                                       original_attrs[attribute.start():attribute.end()])
                if literal:
                    explicit.append(literal[1])
            if re.search(r"\bname\s*=", attrs) and not explicit:
                continue
            name = explicit[-1] if explicit else snake(variant["variant"]).replace("_", "-")
            marked = "ALAB(" in variant[0].partition("//")[2]
            yield enum["enum"], variant["variant"], name, marked


def command_declarations(commit, item, paths):
    """Exact command declaration keys at the artifact/pristine commit, with hook ownership."""
    if commit is None:
        return {}
    camel = "".join(p.capitalize() for p in re.split(r"[-_]", item))
    files = subprocess.run(
        ["git", "-C", REPO, "grep", "-l", "-F", "-e", camel, "-e", '"%s"' % item,
         commit, "--"] + paths, capture_output=True, text=True)
    declarations = {}
    for entry in files.stdout.splitlines():
        path = entry.removeprefix(commit + ":")
        if not path.endswith(".rs"):
            continue
        source = subprocess.run(["git", "-C", REPO, "show", commit + ":" + path],
                                capture_output=True, text=True)
        if source.returncode == 0:
            for enum, variant, name, marked in clap_subcommands(source.stdout):
                if name == item:
                    key = path, enum, variant
                    declarations[key] = None if key in declarations else marked
    return declarations


UPSTREAM = ["codex-rs", ":(exclude)codex-rs/alab-*"]
ALAB = ["codex-rs/alab-*"]


def attribute(side, thing, base, source, vendor_version, paths=UPSTREAM, subcommand=False):
    """The verdict on one item only one side has, and whether it is explained."""
    if base is None:
        return "UNEXPLAINED: no upstream/main in this clone to attribute it (run alab/setup.sh)", False
    if subcommand:
        upstream = command_declarations(base, thing, paths)
        if side == "codex-alab":
            local = command_declarations(source, thing, paths)
            hooks = {key for key, marked in local.items()
                     if marked and key not in upstream}
            owned = command_declarations(source, thing, ALAB)
            unknown = set(local) - set(upstream) - hooks
            ambiguous = any(value is None for declarations in (upstream, local, owned)
                            for value in declarations.values())
            if ambiguous or unknown or ((hooks or owned) and upstream):
                return "UNEXPLAINED: command declarations do not establish one origin", False
            if hooks:
                return "ALab: a marked clap command declaration adds it", True
            if upstream:
                return "upstream: added since vendor %s (in upstream %s)" % (vendor_version, base[:9]), True
            if owned:
                return "ALab: codex-alab's own crates add it", True
            return "UNEXPLAINED: no matching clap command declaration", False
        if not upstream and not named(base, thing, paths):
            return "upstream: gone from upstream %s since vendor %s" % (base[:9], vendor_version), True
        return "UNEXPLAINED: no evidence upstream removed this command", False
    up = named(base, thing, paths)
    if side == "codex-alab":
        if up:
            return "upstream: added since vendor %s (in upstream %s)" % (vendor_version, base[:9]), True
        if named(source, thing, ALAB):
            return "ALab: codex-alab's own crates add it", True
        return "UNEXPLAINED", False
    if not up:
        return "upstream: gone from upstream %s since vendor %s" % (base[:9], vendor_version), True
    return "UNEXPLAINED: upstream still has it, codex-alab does not", False


def help_check(alab, vendor, source, base):
    ta, tv = help_tree(alab), help_tree(vendor)
    out, ok = [], base is not None
    if base is None:
        out.append(("codex", "(upstream/main)", "FAIL",
                    "no upstream/main in %s: nothing to attribute a difference to; run "
                    "alab/setup.sh, then git fetch upstream" % REPO))

    def one_side(where, item, side, thing=None):
        nonlocal ok
        verdict, good = attribute(side, thing or item, base, source, vendor.version,
                                  subcommand=item == "(subcommand)")
        ok &= good
        out.append((where, item, "only in " + side, verdict))

    def mismatch(where, item, why):
        nonlocal ok
        ok = False
        out.append((where, item, "FAIL", why))

    for path in sorted(set(ta) | set(tv)):
        where = " ".join(("codex",) + path)
        if path not in ta or path not in tv:
            one_side(where, "(subcommand)", "codex-alab" if path in ta else "vendor", path[-1])
            continue
        a, v = ta[path], tv[path]
        if a[0] == "ERROR" or v[0] == "ERROR":
            mismatch(where, "--help", "%s / %s" % (a[0] == "ERROR" and a[1], v[0] == "ERROR" and v[1]))
            continue
        (_, ao, aa), (_, vo, va) = a, v
        for name in sorted(set(ao) - set(vo)):
            one_side(where, name, "codex-alab")
        for name in sorted(set(vo) - set(ao)):
            one_side(where, name, "vendor")
        for name in sorted(set(ao) & set(vo)):
            x, y = ao[name], vo[name]
            if x.short != y.short:
                # A short flag codex-alab gained is upstream's when upstream's source spells
                # it; one the vendor has and codex-alab lacks (or spells differently) is a
                # flag someone types, gone.
                if y.short is None and named(base, x.short, UPSTREAM):
                    out.append((where, "%s %s" % (x.short, name), "only in codex-alab",
                                "upstream: added since vendor %s (in upstream %s)"
                                % (vendor.version, base[:9])))
                else:
                    mismatch(where, name, "short flag %s in codex-alab, %s in vendor"
                             % (x.short or "none", y.short or "none"))
            if x.arity != y.arity:
                mismatch(where, name, "takes %s in codex-alab, %s in vendor" % (x.arity, y.arity))
            xv, yv = x.values or set(), y.values or set()
            for value in sorted(xv - yv):
                one_side(where, "%s=%s" % (name, value), "codex-alab", value)
            for value in sorted(yv - xv):
                one_side(where, "%s=%s" % (name, value), "vendor", value)
        if aa != va:
            bare = lambda arg: arg.strip("[]<>.").lower()  # noqa: E731
            an, vn = {bare(x): x for x in aa}, {bare(x): x for x in va}
            for key in sorted(set(an) | set(vn)):
                if key in an and key in vn:
                    if an[key] != vn[key]:
                        mismatch(where, an[key], "argument %s in codex-alab, %s in vendor"
                                 % (an[key], vn[key]))
                elif key in an:
                    one_side(where, an[key], "codex-alab", key)
                else:
                    one_side(where, vn[key], "vendor", key)
            if set(an) == set(vn) and all(an[k] == vn[k] for k in an) and aa != va:
                mismatch(where, "(arguments)", "order %s in codex-alab, %s in vendor" % (aa, va))
    counted = sum(1 for p in ta if ta[p][0] != "ERROR")
    return ok, out, counted


# ---------------------------------------------------------------- exec, app-server, config

def mock_config(port, extra=""):
    return ('model = "gpt-5"\nmodel_provider = "mock"\napproval_policy = "never"\n'
            'sandbox_mode = "read-only"\ncli_auth_credentials_store = "file"\n%s'
            "[model_providers.mock]\nname = \"mock\"\nbase_url = \"http://127.0.0.1:%d/v1\"\n"
            'wire_api = "responses"\nrequires_openai_auth = false\nstream_max_retries = 0\n'
            "request_max_retries = 0\n" % (extra, port))


def exec_check(b, port):
    home = b.fresh_home()
    os.makedirs(home)
    Path(home, "config.toml").write_text(mock_config(port))
    work = os.path.join(b.root, "work")
    os.makedirs(work, exist_ok=True)
    try:
        r = b.run(["exec", "--experimental-json", "--skip-git-repo-check", "--cd", work,
                   "--sandbox", "read-only", PROMPT], codex_home=home, timeout=120)
    except subprocess.TimeoutExpired:
        # A hang detector, never a latency budget: a turn against a local mock takes seconds.
        return False, "no exit within 120 s", home, []
    events = []
    for line in r.stdout.splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            pass
    types = [e.get("type") for e in events]
    said = any("ok" == (e.get("item") or {}).get("text") for e in events)
    good = r.returncode == 0 and said and "turn.completed" in types
    detail = "exit %d, events %s" % (r.returncode, ",".join(dict.fromkeys(t for t in types if t)))
    if not good:
        detail += "; stderr: " + " | ".join(r.stderr.strip().splitlines()[-3:])
    return good, detail, home, events


def event_shapes(events):
    """Every key path the exec JSONL carries, per event type (and item type): what a client
    of `--experimental-json` reads. Values are never compared, only which fields exist."""
    out = set()

    def walk(prefix, value):
        if isinstance(value, dict):
            for key, inner in value.items():
                out.add("%s %s" % (prefix[0], ".".join(prefix[1:] + [key])))
                walk(prefix + [key], inner)
        elif isinstance(value, list):
            for inner in value:
                walk(prefix[:-1] + [prefix[-1] + "[]"] if len(prefix) > 1 else prefix, inner)

    for e in events:
        if not isinstance(e, dict):
            continue
        kind = str(e.get("type"))
        item = e.get("item")
        if isinstance(item, dict) and item.get("type"):
            kind += "[%s]" % item["type"]
        walk([kind], e)
    return out


EXEC_SOURCE = ["codex-rs/exec", "codex-rs/protocol", "codex-rs/app-server-protocol"]


def shape_check(alab, vendor, events, base, source):
    """The fields of the exec JSONL events both binaries emitted, compared."""
    a, v = event_shapes(events["alab"]), event_shapes(events["vendor"])
    rows, ok = [], base is not None
    for side, only in (("codex-alab", sorted(a - v)), ("vendor", sorted(v - a))):
        for path in only:
            field = path.split(" ", 1)[1].split(".")[-1].rstrip("[]")
            verdict, good = attribute(side, field, base, source, vendor.version, EXEC_SOURCE)
            ok &= good
            rows.append(("exec event", path, "only in " + side, verdict))
    return ok, rows, len(a & v)


# ---------------------------------------------------------------- app-server schema

SCHEMA_SOURCE = ["codex-rs/app-server-protocol"]


def alab_notification_addition(item, base, source):
    """Attribute a generated ServerNotification method to its marked protocol declaration."""
    schema, sep, method = item.partition("=")
    if not base or not sep or not method.startswith("alab/") or schema not in (
        "ServerNotification.json method",
        "codex_app_server_protocol.schemas.json ServerNotification.method",
        "codex_app_server_protocol.v2.schemas.json ServerNotification.method",
    ):
        return False
    # Any upstream mention precludes attributing the wire name to ALab here,
    # including an upstream declaration with a different layout or payload.
    upstream = subprocess.run(["git", "-C", REPO, "grep", "-q", "-F", f'"{method}"',
                               base, "--", *SCHEMA_SOURCE], capture_output=True)
    if upstream.returncode != 1:
        return False
    protocol = "codex-rs/app-server-protocol/src/protocol/common.rs"
    read = subprocess.run(["git", "-C", REPO, "show", f"{source}:{protocol}"],
                          capture_output=True, text=True)
    if read.returncode:
        return False
    code = rust_declarations(read.stdout)
    bodies = list(re.finditer(r"^server_notification_definitions!\s*\{\n(?P<body>.*?)^\}",
                              code, re.MULTILINE | re.DOTALL))
    if len(bodies) != 1:
        return False
    body = bodies[0]
    raw_lines = read.stdout[body.start("body"):body.end("body")].splitlines()
    declarations = []
    for masked, raw in zip(body["body"].splitlines(), raw_lines):
        # Masking removes literals and block comments but preserves real line comments.
        row = re.fullmatch(r"\s*\w+\s*=>\s*\(\s*v2::\w+\s*\)\s*,\s*(?://(.*))?", masked)
        if row and re.match(r'\s*\w+\s*=>\s*"' + re.escape(method) + r'"\s*\(', raw):
            declarations.append(bool(re.match(r"\s*ALAB\([\w-]+\):", row[1] or "")))
    return declarations == [True]


@lru_cache(maxsize=256)
def schema_declarations(repo, revision, owner, paths):
    """Read the Rust struct/enum declarations that can own the exported schema name."""
    files = subprocess.run(["git", "-C", repo, "grep", "-l", "-F", "-e", f"struct {owner}",
                            "-e", f"enum {owner}", revision, "--", *paths],
                           capture_output=True, text=True)
    declarations = []
    for entry in files.stdout.splitlines():
        if not entry.endswith(".rs"):
            continue
        source = subprocess.run(["git", "-C", repo, "show", entry], capture_output=True, text=True)
        if source.returncode:
            continue
        code = rust_declarations(source.stdout)
        for decl in re.finditer(r"(?m)^(?P<indent>[ \t]*)(?:pub(?:\([^)]*\))?\s+)?"
                                r"(?:struct|enum)\s+" + re.escape(owner) + r"\s*\{", code):
            tail = code[decl.end():]
            end = re.search(r"(?m)^" + re.escape(decl["indent"]) + "}", tail)
            if end:
                declarations.append(tail[:end.start()])
    return tuple(declarations)


def fork_schema_reason(item, base, source):
    """A fork-only export needs the declared payload in ALab or a marked protocol extension."""
    file, path = item.split(" ", 1)
    name, _, value = path.partition("=")
    if value.startswith("alab/"):
        if alab_notification_addition(item, base, source):
            return "ALab: added by a marked server notification declaration"
        return None
    parts = name.split(".")
    if file == "codex_app_server_protocol.schemas.json" and parts[0] == "v2":
        parts = parts[1:]
    if not parts:
        return None
    # Definitions in a bundle name their owner. Standalone payload fields inherit the file's type.
    if parts[0][:1].isupper():
        owner, members = parts[0], parts[1:]
    else:
        owner, members = os.path.basename(file).removesuffix(".json"), parts
    if not re.fullmatch(r"[A-Z][A-Za-z0-9_]*", owner):
        return None
    upstream = schema_declarations(REPO, base, owner, tuple(SCHEMA_SOURCE))
    if not upstream and len(schema_declarations(REPO, source, owner, tuple(ALAB))) == 1:
        return f"ALab: committed export of ALab's {owner} payload"
    member = value or (members[-1] if members else "")
    if not upstream or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", member):
        return None
    rust_member = member[:1].upper() + member[1:] if value else snake(member)
    # Require the exact new field or enum variant in the existing owner's body and a real
    # adjacent ALAB line comment. Handler arms, another type's fields and prose cannot match.
    declaration = (r"[ \t]*" + re.escape(rust_member) + r"[ \t]*,"
                   if value else r"[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?" + re.escape(rust_member) + r"[ \t]*:[^\n]*?,")
    marker = r"//[ \t]*ALAB\([\w-]+\):"
    if any(re.search("^" + declaration, body, re.MULTILINE) for body in upstream):
        return None
    marked = r"^(?:[ \t]*" + marker + r"[^\n]*\n" + declaration + "|" + declaration + r"[ \t]*" + marker + ")"
    bodies = schema_declarations(REPO, source, owner, tuple(SCHEMA_SOURCE))
    if len(bodies) == 1 and re.search(marked, bodies[0], re.MULTILINE):
        return f"ALab: marked {owner} extension in the committed export"
    return None


def schema_check(alab, vendor, base, source):
    """Compare emitted schema items at their exact file and member paths to all three git archives."""
    actual = {}
    for binary in (alab, vendor):
        out = os.path.join(binary.root, "schema")
        result = binary.run(["app-server", "generate-json-schema", "--experimental", "--out", out])
        if result.returncode != 0 or not os.path.isdir(out):
            return False, [("app-server schema", "--out", "FAIL", f"{binary.label}: exit "
                            f"{result.returncode} {result.stderr.strip()[-200:]}")], 0
        try:
            actual[binary.label] = schema_items(out)
        except (ValueError, OSError) as error:
            return False, [("app-server schema", "(invalid export)", "FAIL", str(error))], 0
        if not actual[binary.label]:
            return False, [("app-server schema", "(empty)", "FAIL", "no definitions read")], 0
    a, v = actual["alab"], actual["vendor"]
    same = len(a & v)
    try:
        pristine = archive_items(REPO, base)
        candidate = archive_items(REPO, source)
        release = archive_items(REPO, f"refs/upstream-tags/rust-v{vendor.version}")
    except (ValueError, OSError) as error:
        return False, [("app-server schema", "(source export)", "FAIL", str(error))], same

    rows = []
    for label, emitted, committed in (("codex-alab", a, candidate), ("vendor", v, release)):
        for direction, items in (("missing from binary", committed - emitted),
                                 ("missing from source", emitted - committed)):
            for item in sorted(items):
                rows.append(("app-server schema", item, direction, f"UNEXPLAINED: {label} differs "
                             "from its exact committed experimental export"))
    ok = not rows
    for side, only in (("codex-alab", a - v), ("vendor", v - a)):
        for item in sorted(only):
            reason = None
            if side == "codex-alab" and item in candidate and item not in release:
                if item in pristine:
                    reason = f"upstream: present in the {base[:9]} export, absent in vendor {vendor.version}"
                else:
                    reason = fork_schema_reason(item, base, source)
            elif side == "vendor" and item in release and item not in pristine and item not in candidate:
                reason = f"upstream: present in vendor {vendor.version} export, absent at {base[:9]}"
            if not reason:
                ok = False
                reason = "UNEXPLAINED: the committed exports and declarations do not establish this difference"
            rows.append(("app-server schema", item, "only in " + side, reason))
    return ok, rows, same


# What the vendor writes into a fresh CODEX_HOME during one exec that codex-alab does not,
# because ALab means it not to (alab/README.md, "Runtime behavior" and "What was removed").
INTENDED_ABSENT = [
    (r"skills/\.system/(imagegen|openai-docs|skill-installer)/",
     "the bundled imagegen, openai-docs and skill-installer skills (alab/removed-paths)"),
    # core-plugins' startup sync: its clone (startup_sync.rs, `plugins-clone-`) and the git
    # scratch dir its git policy makes (git_policy.rs, `git-`).
    (r"\.tmp/(plugins|git-)", "the plugin marketplace sync (`plugins` off, alab-core/defaults.toml)"),
]


# What codex-alab keeps under a name of its own in the CODEX_HOME it shares with codex-old
# (codex-rs/alab-core/src/home.rs), as (codex-alab's prefix, the vendor's): the same files as the
# vendor's, so the comparison reads them under the vendor's names, and codex-alab writing under
# the vendor's name is a clash with codex-old, never a shared path.
OWN_NAMES = [
    ("skills/.alab-system/", "skills/.system/"),
    ("models_cache.alab.", "models_cache."),
]


def as_vendor_names(paths):
    """codex-alab's shapes under the vendor's names (OWN_NAMES), and the paths codex-alab wrote
    under one of those names itself."""
    renamed, clashes = set(), []
    for path in paths:
        own = next(((o, v) for o, v in OWN_NAMES if path.startswith(o)), None)
        if own:
            path = own[1] + path[len(own[0]):]
        elif any(path.startswith(v) for _, v in OWN_NAMES):
            clashes.append(path)
        renamed.add(path)
    return renamed, sorted(clashes)


def new_bundled_skills(home, tag, base, source):
    """Attribute files added to the upstream embedded skill tree only at their installed bytes."""
    assets = "codex-rs/skills/src/assets/samples/"
    if not base or not source:
        return set(), []
    delta = subprocess.run(["git", "-C", REPO, "diff", "--no-renames", "--diff-filter=A",
                            "--name-only", "-z", tag, base, "--", assets], capture_output=True)
    if delta.returncode:
        return set(), []  # No attribution. Any observed extra still fails the layout check.
    confirmed, unconfirmed = set(), []
    root = Path(home).resolve() / "skills/.alab-system"
    for name in filter(None, os.fsdecode(delta.stdout).split("\0")):
        relative = name.removeprefix(assets)
        observed = "skills/.system/" + relative
        path = root / relative
        blobs = [subprocess.run(["git", "-C", REPO, "show", f"{rev}:{name}"], capture_output=True)
                 for rev in (base, source)]
        if (all(blob.returncode == 0 for blob in blobs) and path.resolve() == path and path.is_file()
                and blobs[0].stdout == blobs[1].stdout == path.read_bytes()):
            confirmed.add(observed)
        else:
            unconfirmed.append(observed)
    return confirmed, unconfirmed


def shapes(home):
    """The files under CODEX_HOME, digits and ids folded so two runs compare by shape. A
    SQLite -wal/-shm pair is the database's own transient state (present while a process
    still holds it open), never part of the layout."""
    out = set()
    for d, _, files in os.walk(home):
        for f in files:
            if f.endswith(("-wal", "-shm")):
                continue
            rel = os.path.relpath(os.path.join(d, f), home)
            if rel.startswith("skills/"):
                out.add(rel)  # Embedded asset names are literal, even when they contain digits.
                continue
            rel = re.sub(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", "<id>", rel)
            if rel.startswith(("tmp/", ".tmp/")):
                # A scratch dir's random suffix (mkdtemp's, tempfile's): `git-keiNSZ`.
                rel = re.sub(r"(?<=-)[A-Za-z0-9]{6,}(?=/|$)", "<rand>", rel)
            out.add(re.sub(r"\d+", "N", rel))
    return out


def app_server(b, requests, codex_home=None, extra_env=None):
    """`codex app-server --listen stdio://` (the Python SDK's argv): initialize, then each
    request in turn. Returns {id: reply} for the requests answered within 30 s each."""
    home = codex_home or b.fresh_home()
    init = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"clientInfo": {"name": "dropin", "title": "dropin", "version": "0"},
                       "capabilities": {"experimentalApi": True}}}
    replies = {}
    with ExitStack() as cleanup:
        p = cleanup.enter_context(subprocess.Popen(
            [b.exe, "app-server", "--listen", "stdio://"],
            env={**b.env(home), **(extra_env or {})},
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, cwd=b.root,
        ))
        cleanup.callback(p.kill)
        for n, (method, params) in enumerate([("initialize", None)] + list(requests), start=1):
            msg = init if method == "initialize" else {"jsonrpc": "2.0", "id": n, "method": method,
                                                       "params": params}
            p.stdin.write(json.dumps(msg) + "\n")
            if method == "initialize":
                p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "initialized"}) + "\n")
            p.stdin.flush()
            # A hang detector, never a latency budget.
            deadline = time.time() + 30
            while time.time() < deadline and n not in replies:
                ready, _, _ = select.select([p.stdout], [], [], 1)
                if not ready:
                    continue
                line = p.stdout.readline()
                if not line:
                    return replies
                try:
                    reply = json.loads(line)
                except ValueError:
                    continue
                if reply.get("id") == n:
                    replies[n] = reply
            if n not in replies:
                return replies
    return replies


def app_server_check(b):
    reply = app_server(b, []).get(1)
    good = bool(reply) and "result" in reply
    ua = ((reply or {}).get("result") or {}).get("userAgent", "")
    return good, ("initialize answered (userAgent %s)" % ua.split(" ")[0]) if good else "no initialize result: %s" % reply


# ---------------------------------------------------------------- effective config

def flatten(value, prefix=""):
    """A config as {dotted key: leaf}, a list or an empty table being one leaf."""
    if isinstance(value, dict) and value:
        out = {}
        for key, inner in value.items():
            out.update(flatten(inner, "%s.%s" % (prefix, key) if prefix else key))
        return out
    return {prefix: value}


def toml_at(commit, path):
    """The TOML file `path` at `commit`, flattened; {} when the commit has none."""
    import tomllib
    r = subprocess.run(["git", "-C", REPO, "show", "%s:%s" % (commit, path)],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return None
    return flatten(tomllib.loads(r.stdout))


def effective_config(b, extra_env=None, config=None):
    home = b.fresh_home()
    os.makedirs(home)
    if config:
        Path(home, "config.toml").write_text(config)
    reply = app_server(b, [("config/read", {})], codex_home=home, extra_env=extra_env).get(2) or {}
    config = (reply.get("result") or {}).get("config")
    return flatten(config) if isinstance(config, dict) else None


def fresh_thread_effort_check(b, expected):
    replies = app_server(b, [("thread/start", {"cwd": b.root, "ephemeral": True})],
                         extra_env={"OPENAI_BASE_URL": "http://127.0.0.1:1/backend-api/codex"})
    result = (replies.get(2) or {}).get("result") or {}
    actual = result.get("reasoningEffort")
    return actual == expected, f"thread/start returned {actual!r}; source defaults declare {expected!r}"


def own_key(key):
    """ALab's own config key (`[tui] alab_progress`): its name starts with `alab_`, a prefix
    upstream's config never uses, so an upstream rename can never land on it."""
    return key.rsplit(".", 1)[-1].startswith("alab_")


@lru_cache(maxsize=12)
def registry_defaults(repo, commit):
    """Read literal boolean defaults in unconditional entries of upstream's FEATURES array."""
    result = subprocess.run(["git", "-C", repo, "show", f"{commit}:codex-rs/features/src/lib.rs"],
                            capture_output=True, text=True)
    if result.returncode:
        return {}
    source = result.stdout
    code = re.sub(r"//[^\n]*", lambda match: " " * len(match[0]), rust_declarations(source))
    tables = list(re.finditer(r"^pub const FEATURES: &\[FeatureSpec\] = &\[\n(.*?)^\];", code, re.M | re.S))
    if len(tables) != 1:
        return {}
    table, = tables
    defaults = {}
    for spec in re.finditer(r"(?m)^(?P<attrs>(?:    #\[[^\n]*\]\n)*)    FeatureSpec \{\n"
                            r"(?P<body>.*?)^    \},", table[1], re.S):
        body = spec["body"]
        key = re.search(r"^        key: ( +),$", body, re.M)
        if not key:
            continue
        offset = table.start(1) + spec.start("body")
        literal = source[offset + key.start(1):offset + key.end(1)]
        if not re.fullmatch(r'"[a-z0-9_]+"', literal):
            continue
        name = literal[1:-1]
        ident = re.findall(r"^        id: Feature::(\w+),$", body, re.M)
        value = re.findall(r"^        default_enabled: (true|false),$", body, re.M)
        plain = len(ident) == len(value) == 1 and not spec["attrs"] and "#[" not in body
        defaults[name] = (ident[0], value[0] == "true") if plain and name not in defaults else None
    return defaults


def upstream_feature_flip(key, previous, current, tag, base, source):
    if not key.startswith("features.") or type(previous) is not bool or type(current) is not bool or previous == current:
        return False
    old, new, fork = (registry_defaults(REPO, rev).get(key.removeprefix("features."))
                      for rev in (tag, base, source))
    return bool(old and new and new == fork and old[0] == new[0] and old[1] == previous and new[1] == current)


def config_check(alab, vendor, source, base, in_aterm):
    """The layering law, measured: codex-alab with no config.toml reads exactly what the
    vendor reads with ALab's packaged defaults (alab-core/defaults.toml, and
    aterm-defaults.toml over it inside aterm, at the package's source commit) as its
    config.toml. So every ALab choice is a plain config value a user's config.toml can
    override, and nothing else differs but upstream's own moves since the vendor's
    release."""
    env = {"TERM_PROGRAM": "aterm"} if in_aterm else None
    where = "config (%s)" % ("TERM_PROGRAM=aterm" if in_aterm else "no aterm")
    ours = toml_at(source, "codex-rs/alab-core/defaults.toml")
    if not ours:
        return False, [(where, "defaults.toml", "FAIL", "no alab-core/defaults.toml at %s" % source[:9])], 0
    if in_aterm:
        ours.update(toml_at(source, "codex-rs/alab-core/aterm-defaults.toml") or {})
    as_config = "".join("%s = %s\n" % (key, json.dumps(value)) for key, value in sorted(ours.items()))
    a = effective_config(alab, env)
    v = effective_config(vendor, env, as_config)
    if not a or not v:
        return False, [(where, "config/read", "FAIL", "no config read (codex-alab %s, vendor %s)"
                        % (bool(a), bool(v)))], 0
    tag = "refs/upstream-tags/rust-v%s" % vendor.version
    tagged = subprocess.run(["git", "-C", REPO, "rev-parse", "-q", "--verify", tag],
                            capture_output=True).returncode == 0
    then = (toml_at(tag, "codex-rs/config/defaults.toml") or {}) if tagged else None
    now = (toml_at(base, "codex-rs/config/defaults.toml") or {}) if base else None
    rows, ok = [], True
    for key, value in sorted(ours.items()):
        if own_key(key):
            # ALab's own key: upstream's typed config has no such field, so config/read
            # shows it in neither build; codex-alab reads it from the config layers
            # (alab-tui's tests pin that). Shown here, it would be a field upstream added.
            if a.get(key) is not None or v.get(key) is not None:
                ok = False
                rows.append((where, key, "FAIL", "ALab's own key in config/read (codex-alab %s,"
                             " vendor %s): upstream has a field by its name"
                             % (json.dumps(a.get(key)), json.dumps(v.get(key)))))
            else:
                rows.append((where, key, "ALab's own", "not a config/read field in either build;"
                             " read from the layers (alab-tui tests)"))
            continue
        if a.get(key) != value:
            ok = False
            rows.append((where, key, "FAIL", "ALab's default %s is not in effect (%s)"
                         % (json.dumps(value), json.dumps(a.get(key)))))
    for key in sorted(set(a) | set(v)):
        if a.get(key) == v.get(key):
            continue
        if then is not None and now is not None and then.get(key) != now.get(key) \
                and now.get(key) == a.get(key):
            verdict = "upstream: its config/defaults.toml moved since vendor %s" % vendor.version
        elif (tagged and base and key not in ours and key not in (then or {}) and key not in (now or {})
              and upstream_feature_flip(key, v.get(key), a.get(key), tag, base, source)):
            verdict = (f"upstream: the feature registry default changed from {json.dumps(v[key])} "
                       f"in vendor {vendor.version} to {json.dumps(a[key])} at {base[:9]}")
        elif v.get(key) is None and named(base, key.split(".")[-1], UPSTREAM):
            verdict = "upstream: a key added since vendor %s (in upstream %s)" % (
                vendor.version, base[:9])
        else:
            ok = False
            verdict = "UNEXPLAINED: codex-alab %s, vendor with ALab's defaults %s%s" % (
                json.dumps(a.get(key)), json.dumps(v.get(key)),
                "" if tagged else " (no %s to read upstream's defaults at)" % tag)
        rows.append((where, key, "differs", verdict))
    return ok, rows, len(ours)


def override_check(alab, source):
    """Each ALab feature default flips back the three ways a codex user flips a feature."""
    features = {k.split(".", 1)[1]: val for k, val in
                (toml_at(source, "codex-rs/alab-core/defaults.toml") or {}).items()
                if k.startswith("features.") and isinstance(val, bool)}
    if not features:
        return False, "no [features] read from alab-core/defaults.toml at %s" % source[:9]
    wrong = []
    for name, value in sorted(features.items()):
        flipped = "false" if value else "true"
        ways = {"config.toml": feature(alab, name, "[features]\n%s = %s\n" % (name, flipped)),
                "-c": feature(alab, name, args=["-c", "features.%s=%s" % (name, flipped)]),
                "--enable/--disable": feature(alab, name, args=["--disable" if value else "--enable", name])}
        if feature(alab, name) != str(value).lower():
            wrong.append("%s: default %s, not ALab's %s" % (name, feature(alab, name), value))
        wrong += ["%s via %s: %s" % (name, way, got) for way, got in ways.items() if got != flipped]
    return not wrong, ("%d ALab feature defaults (%s), each flipped by config.toml, -c and "
                       "--enable/--disable" % (len(features), ", ".join(sorted(features)))
                       if not wrong else "; ".join(wrong))


def feature(b, name, config=None, args=()):
    home = b.fresh_home()
    os.makedirs(home)
    if config:
        Path(home, "config.toml").write_text(config)
    r = b.run(list(args) + ["features", "list"], codex_home=home)
    for line in r.stdout.splitlines():
        cols = line.split()
        if cols and cols[0] == name:
            return cols[-1]
    return "absent (exit %d)" % r.returncode


def mcp_check(b):
    """An MCP server a config.toml declares is listed as declared (`codex mcp list --json`)."""
    home = b.fresh_home()
    os.makedirs(home)
    Path(home, "config.toml").write_text(
        '[mcp_servers.dropin]\ncommand = "/usr/bin/true"\nargs = ["--stdio"]\n')
    r = b.run(["mcp", "list", "--json"], codex_home=home)
    try:
        listed = json.loads(r.stdout)
    except ValueError:
        return False, "exit %d, not JSON: %s" % (r.returncode, r.stdout.strip()[:120])
    names = [s.get("name") for s in listed] if isinstance(listed, list) else []
    return r.returncode == 0 and names == ["dropin"], "exit %d, servers %s" % (r.returncode, names)


def login_check(b):
    home = b.fresh_home()
    os.makedirs(home)
    Path(home, "config.toml").write_text('cli_auth_credentials_store = "file"\n')
    r = b.run(["login", "--with-api-key"], codex_home=home, stdin="dropin-placeholder\n")
    s = b.run(["login", "status"], codex_home=home)
    files = sorted(os.listdir(home))
    good = r.returncode == 0 and s.returncode == 0 and "auth.json" in files
    return good, "login exit %d, status exit %d (%s), CODEX_HOME holds %s" % (
        r.returncode, s.returncode, (s.stdout + s.stderr).strip().splitlines()[0] if (s.stdout + s.stderr).strip() else "", ", ".join(files))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--alab")
    ap.add_argument("--vendor")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    alab_pkg = os.path.realpath(args.alab or newest_alab())
    vendor_pkg = os.path.realpath(args.vendor or newest_vendor())
    if not os.path.isfile(os.path.join(alab_pkg, "alab-build.json")):
        sys.exit(f"dropin: {alab_pkg} carries no alab-build.json: not a codex-alab package")
    if os.path.exists(os.path.join(vendor_pkg, "alab-build.json")):
        sys.exit(f"dropin: {vendor_pkg} carries alab-build.json: not the vendor's codex")
    with open(os.path.join(alab_pkg, "alab-build.json")) as f:
        manifest = json.load(f)
    source = manifest["source"]
    try:
        base = checked_upstream_base(source, manifest)
    except ValueError as error:
        sys.exit(f"dropin: {error}")
    with ExitStack() as cleanup:
        scratch = tempfile.mkdtemp(prefix="alab-dropin-")
        if not args.keep:
            register_directory_cleanup(cleanup, scratch)
        return compare_packages(alab_pkg, vendor_pkg, source, base, scratch)


def compare_packages(alab_pkg, vendor_pkg, source, base, scratch):
    alab, vendor = Bin("alab", alab_pkg, scratch), Bin("vendor", vendor_pkg, scratch)
    print("codex-alab:", alab.exe, alab.version, "source", source[:9])
    print("vendor:    ", vendor.exe, vendor.version)
    print("scratch:   ", scratch)
    results, ok = [], True

    def record(name, good, detail):
        nonlocal ok
        ok &= bool(good)
        results.append((name, "PASS" if good else "FAIL", detail))

    for b in (alab, vendor):
        v = b.run(["--version"]).stdout.strip()
        record("name (%s)" % b.label,
               b.entry == "bin/codex" and os.path.basename(b.exe) == "codex" and v == "codex-cli " + b.version,
               "entrypoint %s, --version '%s'" % (b.entry, v))

    good, diffs, counted = help_check(alab, vendor, source, base)
    record("help tree", good, "%d help pages; %d differences, attributed below (upstream base %s)"
           % (counted, len(diffs), base and base[:9]))

    homes, events = {}, {}
    with serve(Mock()) as server:
        try:
            for b in (alab, vendor):
                good, detail, homes[b.label], events[b.label] = exec_check(b, server.server_address[1])
                record("exec --experimental-json (%s)" % b.label, good, detail)
        finally:
            server.shutdown()
    (sa, clashes), sv = as_vendor_names(shapes(homes["alab"])), shapes(homes["vendor"])
    confirmed, unconfirmed = new_bundled_skills(homes["alab"], f"refs/upstream-tags/rust-v{vendor.version}",
                                               base, source)
    vendor_clashes = sorted(confirmed & sv)
    extra = sorted(sa - sv - confirmed)
    missing = {}
    for path in sorted(sv - sa):
        why = next((w for pat, w in INTENDED_ABSENT if re.match(pat, path)), None)
        missing.setdefault(why, []).append(path)
    unexplained = missing.pop(None, [])
    record("CODEX_HOME after exec", not any((extra, unexplained, clashes, unconfirmed, vendor_clashes)),
           "%d paths in both (%s; codex-alab's own names read as the vendor's: %s)%s%s" % (
               len(sa & sv), ", ".join(sorted(p for p in sa & sv if "/" not in p or p.startswith("sessions/"))[:8]),
               ", ".join("%s for %s" % pair for pair in OWN_NAMES),
               "".join("; not written, as ALab intends (%d): %s" % (len(v), w) for w, v in missing.items()),
               ("; upstream-added bundled skill files at %s: %s" % (base[:9], sorted(confirmed)) if confirmed else "")
               + ("; missing or changed upstream skill files: %s" % unconfirmed if unconfirmed else "")
               + ("; vendor wrote skill files absent at its tag: %s" % vendor_clashes if vendor_clashes else "")
               + ("; ONLY codex-alab: %s" % extra if extra else "")
               + ("; UNEXPLAINED vendor-only: %s" % unexplained if unexplained else "")
               + ("; codex-alab wrote into codex-old's own: %s" % clashes if clashes else "")))
    good, rows, same = shape_check(alab, vendor, events, base, source)
    diffs += rows
    record("exec event fields", good, "%d key paths in both; %d differences, attributed below"
           % (same, len(rows)))

    for b in (alab, vendor):
        good, detail = app_server_check(b)
        record("app-server stdio initialize (%s)" % b.label, good, detail)
    good, rows, same = schema_check(alab, vendor, base, source)
    diffs += rows
    record("app-server schema", good, "%d definitions, properties and enum values in both; "
           "%d differences, attributed below" % (same, len(rows)))

    for in_aterm in (False, True):
        good, rows, same = config_check(alab, vendor, source, base, in_aterm)
        diffs += rows
        record("effective config (%s)" % ("TERM_PROGRAM=aterm" if in_aterm else "no aterm"), good,
               "%d ALab defaults, each in effect and each read by the vendor as a plain config.toml "
               "value; %d other differences, attributed below" % (same, len(rows)))
    packaged = toml_at(source, "codex-rs/alab-core/defaults.toml") or {}
    if "model_reasoning_effort" in packaged:
        good, detail = fresh_thread_effort_check(alab, packaged["model_reasoning_effort"])
        record("fresh thread reasoning effort", good, detail)
    good, detail = override_check(alab, source)
    record("ALab defaults overridable", good, detail)

    for b in (alab, vendor):
        good, detail = mcp_check(b)
        record("mcp list (%s)" % b.label, good, detail)

    for b in (alab, vendor):
        good, detail = login_check(b)
        record("auth file (%s)" % b.label, good, detail)

    width = max(len(r[0]) for r in results)
    print()
    for name, verdict, detail in results:
        print("%-*s  %s  %s" % (width, name, verdict, detail))
    if diffs:
        print("\ndifferences (codex-alab vs vendor %s):" % vendor.version)
        for where, item, side, verdict in diffs:
            print("  %-28s %-26s %-17s %s" % (where, item, side, verdict))
    print("\ndropin: %s" % ("PASS" if ok else "FAIL"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
