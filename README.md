# steplock

**Stateful quality gates for AI coding agent actions.**

steplock sits on top of the [polyhook](https://github.com/tupe12334/polyhook) SDK. It intercepts hook events and gates actions behind a sequential checklist — one question per invocation, acknowledged by the agent, persisted to disk.

**[Installation →](Installation.md)** · **[Architecture →](Architecture.md)** · **[Contributing →](CONTRIBUTING.md)**

---

## The Problem

AI coding agents act fast. They run commands, push code, and make changes — but they have no built-in mechanism to enforce quality gates before doing so. Every hook invocation starts from scratch: no memory of what was already checked, no way to hold the agent accountable to a sequential process.

You want the agent to answer "did you increase test coverage?" before every push — not just the first time, but every time. You want it to work regardless of which AI tool the agent runs in. And you want to define that requirement once, declaratively, without writing stateful logic yourself.

steplock solves this. polyhook is the transport layer that normalizes hook events across AI tools (Claude Code, Cursor, Windsurf, Cline, Amp, and others) — steplock sits on top and adds the stateful gate logic.

---

## How It Works

```
agent tries action (e.g. git push)
        │
steplock reads state file
        │
   unchecked items?
   ┌────┴────┐
  yes        no
   │          │
block        approve
agent with   action
next item
   │
agent acknowledges
   │
state file updated
   │
agent retries → next item shown
   │
... until all items checked
   │
action approved
```

Each checklist item requires one full hook invocation to acknowledge. The agent cannot skip ahead.

---

## Configuration

Each checklist lives in its own directory under `.steplock/checklists/<name>/`. The directory name is the checklist identifier. Two files per checklist:

```
.steplock/
└── checklists/
    └── git-push-quality-gate/
        ├── config.toml
        └── flow.mmd
```

**`config.toml`** — trigger and behaviour. See [`schemas/checklist-config.schema.json`](schemas/checklist-config.schema.json).

**`flow.mmd`** — the [Mermaid `stateDiagram-v2`](https://mermaid.js.org/syntax/stateDiagram.html) diagram. State labels are the questions shown to the agent. steplock walks the graph — on each block it shows the label of the current state; on ack it advances to the next state. `[*]` marks start and end.

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

Branching — multiple transitions from one state. steplock prompts the agent with options:

```
stateDiagram-v2
    [*] --> clean_code
    clean_code --> test_coverage
    clean_code --> skip_reason

    test_coverage --> [*]
    skip_reason   --> [*]

    clean_code   : Did you write clean, readable code?
    test_coverage: Did you increase test coverage?
    skip_reason  : Describe why test coverage was skipped.
```

See [`examples/git-push-quality-gate/`](examples/git-push-quality-gate/) for a complete working example.

---

## Editor support

**`config.toml`** — add the `#:schema` hint. [Taplo](https://taplo.tamasfe.dev) (via [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml)) provides autocomplete and inline validation.

**`flow.mmd`** — `.mmd` extension gives Mermaid syntax highlighting and diagram preview in VS Code via the [Mermaid Preview](https://marketplace.visualstudio.com/items?itemName=bierner.markdown-mermaid) extension.

Both extensions are listed in [`.vscode/extensions.json`](.vscode/extensions.json) — VS Code will prompt to install them on repo open.
