# keidocs

Auto-extracts file-level documentation (rustdoc `///`+`//!`, jsdoc `/** */`,
markdown headers) and emits one `.md` per source file with DNA frontmatter
(content-addressable hash) so downstream tools can detect stale docs.

## Use

```
keidocs extract --root <src-dir> --out <docs-dir>
keidocs validate --out <docs-dir>
```

`extract` walks the tree (skipping `target/`, `node_modules/`, `.git/`),
detects language by extension (`.rs` / `.ts` / `.tsx` / `.js` / `.jsx` /
`.md`), parses comments, computes the DNA hash, and writes flattened
markdown files (e.g. `src/lib.rs` → `src__lib.rs.md`).

`validate` re-reads the output and confirms every file has both a
`dna_hash:` frontmatter key and a `- parent:` backlink. Exits non-zero
on mismatch — wire it into pre-commit to catch hand-edited drift.
