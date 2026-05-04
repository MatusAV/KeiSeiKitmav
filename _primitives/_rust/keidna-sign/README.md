# keidna-sign

Signed DNA manifest for KeiSeiKit primitives. Phase 1 = sha256 content
hashing; Phase 2 (planned) layers ed25519 attestation on top of `dna_hash`.

## What it does

Walks `src/**/*.rs` + `Cargo.toml` of a primitive, hashes each file with
sha256, then computes an aggregate `dna_hash` over `(name, version,
sorted file hashes, sorted deps)`. Output goes to `<primitive>/.dna.json`.

Order-independent in `[dependencies]`; deterministic across machines.

## CLI

```bash
# emit DNA for one primitive (root = workspace dir containing the primitive)
keidna-sign emit --primitive kei-cortex --root _primitives/_rust

# verify .dna.json still matches current source
keidna-sign verify --primitive kei-cortex --root _primitives/_rust

# table of all primitives + their stored DNA
keidna-sign list --root _primitives/_rust
```

`GIT_COMMIT` and `GIT_AUTHOR` env vars feed the manifest fields when set.

## Use in CI

```yaml
- run: keidna-sign verify --primitive kei-cortex --root _primitives/_rust
  # exit 1 if source diverged from committed .dna.json
```

## Sample `.dna.json`

```json
{
  "name": "kei-cortex",
  "version": "0.1.0",
  "dna_hash": "sha256:7b1c…",
  "files": [
    {"path": "Cargo.toml", "sha256": "…"},
    {"path": "src/main.rs", "sha256": "…"}
  ],
  "deps": ["anyhow", "axum", "tokio"],
  "generated": "2026-05-04T01:30:00Z",
  "git_commit": "abc123",
  "author": "Denis Parfionovich",
  "lineage": {"parent_dna": null, "fork_of": null}
}
```

Phase 2: `signature` field over canonical `dna_hash` via ed25519
(see sibling `kei-ledger-sign`).
