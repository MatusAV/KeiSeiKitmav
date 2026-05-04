# GitHub Actions Workflows

| Workflow | Trigger | Purpose |
|---|---|---|
| `ci.yml` | push, PR | Run cargo / npm tests across the workspace |
| `leak-check.yml` | push, PR | Scan for accidental secrets / patent-IP terms |
| `release.yml` | tag `v*` | Build release artefacts + GitHub Release |
| `keiwiki.yml` | push to `main` (paths: `_primitives/**`, `skills/**`, `hooks/**`, `docs-site/**`, this workflow) + manual | Build keidocs, extract DNA-tagged docs, render Astro Starlight site, deploy to GitHub Pages |

## Triggering manually

```bash
gh workflow run keiwiki.yml --ref main
```

Or in the GitHub UI: **Actions → KeiWiki Build & Deploy → Run workflow**.

## KeiWiki one-time setup

Repo **Settings → Pages → Build and deployment → Source = "GitHub Actions"**.

The workflow's `deploy` job needs `pages: write` + `id-token: write` permissions
(declared at the top of `keiwiki.yml`) and the auto-provided
`secrets.GITHUB_TOKEN`. No additional secrets required.

## Local equivalents

See `_primitives/templates/keiwiki-justfile-recipe.txt` for a `just docs` recipe
that mirrors the workflow's build steps.
