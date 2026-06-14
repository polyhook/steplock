# Examples

## git-push-quality-gate

A four-item quality checklist that gates `git push` commands.

**Trigger**: `tool:before` on `bash` when `input.command.contains('git push')`.

**Flow**: linear — four states in sequence.

```
[*] → clean_code → test_coverage → documentation → no_secrets → [*]
```

Each state blocks the agent until acknowledged. On the fifth invocation, the push is approved.

**Preview enabled**: `allow_preview_request = true` — on the first block, the agent sees a tip to run `preview.sh` and view all four items before starting.

### Files

```
git-push-quality-gate/
├── .claude/settings.json                        ← Claude Code hook registration
└── .steplock/
    ├── .gitignore                               ← excludes sessions/ and audit.log
    └── checklists/git-push-quality-gate/
        ├── config.toml                          ← trigger and behaviour
        └── flow.mmd                             ← checklist items as Mermaid diagram
```

### Usage

Copy `git-push-quality-gate/.steplock/` into your repo root and register the hook per your AI tool's documentation (see [`Installation.md`](../Installation.md)).

---

## sensitive-file-guard

A single-item confirmation gate that fires **every time** the agent tries to read a file that may contain secrets: `/etc/*`, `*.env`, `*.key`, `*.pem`.

**Trigger**: `tool:before` on `read_file` when the path matches.

**Flow**: one state — just a confirmation before reading.

```
[*] → confirm_read → [*]
```

**Reset**: `always` — the gate fires on every matching read, not once per session. There is no persistent state file.

### Files

```
sensitive-file-guard/
├── .claude/settings.json                         ← Claude Code hook registration (Read tool)
└── .steplock/
    ├── .gitignore                                ← excludes sessions/ and audit.log
    └── checklists/sensitive-file-guard/
        ├── config.toml                           ← trigger and behaviour (reset = "always")
        └── flow.mmd                              ← single-item confirmation
```

### Usage

Copy `sensitive-file-guard/.steplock/` into your repo root and register the hook for the `Read` tool (or your AI tool's equivalent file-read action).
