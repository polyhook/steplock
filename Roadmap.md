# Roadmap

## Homebrew tap

Publish a `polyhook/homebrew-tap` tap so users can install via:

```sh
brew install polyhook/tap/steplock
```

---

## Shared flows

No registry. A `polyhook/steplock-flows` GitHub repo holds community flows. `steplock add <user>/<flow>` fetches and **inlines** the flow directly into `steplock.toml`. Once inlined there is no runtime network dependency — the flow is reproducible and works offline. A live URL reference would silently change under you.
