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
