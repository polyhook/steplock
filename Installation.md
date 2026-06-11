# Installation

## Prerequisites

- [`jq`](https://jqlang.github.io/jq/) — required at runtime by `ack.sh`
- [`polyhook`](https://github.com/polyhook/polyhook) — the underlying hook transport
- An AI coding tool that fires hooks (Claude Code, Cursor, Windsurf, Cline, Amp, or any polyhook-compatible tool)

Install `jq` if missing:

```sh
# macOS
brew install jq

# Debian/Ubuntu
apt-get install jq

# Windows (via Chocolatey)
choco install jq
```

---

## Install steplock

### Binary (recommended)

Download the latest release for your platform from [GitHub Releases](https://github.com/polyhook/steplock/releases) and place it on your `PATH`:

```sh
# macOS/Linux — example, adjust version and platform
curl -L https://github.com/polyhook/steplock/releases/latest/download/steplock-$(uname -s)-$(uname -m).tar.gz | tar -xz
mv steplock /usr/local/bin/
```

Verify:

```sh
steplock --version
```

### Build from source

Requires [Rust](https://rustup.rs) (stable).

```sh
git clone https://github.com/polyhook/steplock.git
cd steplock
cargo build --release
mv target/release/steplock /usr/local/bin/
```

### Package managers

```sh
# Cargo
cargo install steplock-core
```

---

## Project setup

Run inside your repo root. Creates the `.steplock/checklists/` directory tree:

```sh
steplock init
```

Or create manually:

```sh
mkdir -p .steplock/checklists
```

Add to `.gitignore` — commit checklists, ignore runtime state:

```
.steplock/sessions/
.steplock/audit.log
```

---

## Create a checklist

Each checklist lives in its own subdirectory. The directory name is the checklist identifier.

```
.steplock/
└── checklists/
    └── git-push-quality-gate/
        ├── config.toml
        └── flow.mmd
```

**`config.toml`** — trigger and behaviour:

```toml
#:schema https://raw.githubusercontent.com/polyhook/steplock/refs/heads/main/schemas/checklist-config.schema.json

on_event = "tool:before"
on_tool   = "bash"
match_input = "input.command.contains('git push')"
reset     = "session"
allow_preview_request = true
```

**`flow.mmd`** — checklist items as a Mermaid state diagram:

```
stateDiagram-v2
    [*] --> clean_code
    clean_code --> test_coverage
    test_coverage --> documentation
    documentation --> no_secrets
    no_secrets --> [*]

    clean_code   : Did you write clean, readable code?
    test_coverage: Did you increase test coverage by at least a little?
    documentation: Did you update relevant documentation?
    no_secrets   : Did you check for hardcoded secrets or credentials?
```

See [`examples/git-push-quality-gate/`](examples/git-push-quality-gate/) for a working reference.

---

## Register the hook

Tell your AI tool to invoke `steplock` on hook events. Configuration varies by tool.

### Claude Code

In your project's `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "steplock" }
        ]
      }
    ]
  }
}
```

### Cursor / Windsurf / Cline / Amp

Follow your tool's hook registration docs and point the hook command at `steplock`. polyhook normalises the event format — no per-tool changes needed.

---

## Verify

Trigger a matched action from your AI tool (e.g. ask it to run `git push`). Expected output in the hook response:

```
BLOCKED — Did you write clean, readable code?
When finished, run: sh .steplock/sessions/<session-id>/ack.sh
Then retry your original command.
```

Check the audit log for a machine-readable record:

```sh
cat .steplock/audit.log
```

---

## Editor support

Add the schema hint to `config.toml` (already shown above) for [Taplo](https://taplo.tamasfe.dev) autocomplete and inline validation via the [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml) VS Code extension.

For `flow.mmd`, install [Mermaid Preview](https://marketplace.visualstudio.com/items?itemName=bierner.markdown-mermaid) to get syntax highlighting and diagram preview.

Both extensions are listed in [`.vscode/extensions.json`](.vscode/extensions.json) — VS Code prompts to install them on repo open.
