# KeiSeiKit Docs Site

Astro 5 + Starlight build of the living KeiSeiKit wiki.

## Develop

```bash
cd docs-site
npm install
npm run dev
```

Dev server runs at http://localhost:4321.

## Build

```bash
npm run build
```

Static output is written to `../site/` (i.e. `KeiSeiKit-public/site/`),
ready to be served by Caddy / nginx / a CDN.

## Auto-generated content

The `Primitives`, `Skills`, and `Hooks` sidebar sections are populated
from `docs/{primitives,skills,hooks}/*.md` (top-level `docs/`
directory), which is produced by the `keidocs` Rust primitive on every
commit. To rebuild docs and site together:

```bash
keidocs build           # regenerates docs/*.md from source
cd docs-site
npm run build           # produces ../site/
```

The `Overview` section is hand-written narrative under
`src/content/docs/overview/`.
