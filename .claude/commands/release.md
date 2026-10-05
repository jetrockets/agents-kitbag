# Release Versioning — Claude Toolkit

Project is in **beta**. All versions use `0.x.y` scheme until stable release.

## Version Format

`0.MINOR.PATCH` — no major version until project exits beta.

## When to Bump What

| Change type | Bump | Example |
|-------------|------|---------|
| New feature or integration | **minor** | v0.15.0 → v0.16.0 |
| Bug fix, docs, refactoring | **patch** | v0.16.0 → v0.16.1 |
| Breaking change | **minor** (with note in release) | v0.16.1 → v0.17.0 |
| Stable public release | **1.0.0** | Only when explicitly decided |

## CHANGELOG.md

Project maintains `CHANGELOG.md` following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

### Entry Types

- **Added** — new features
- **Changed** — changes in existing functionality
- **Deprecated** — soon-to-be removed features
- **Removed** — now removed features
- **Fixed** — bug fixes
- **Security** — vulnerability fixes

### CHANGELOG Rules

- Every release MUST have an entry in `CHANGELOG.md`
- Keep an `[Unreleased]` section at the top for upcoming changes
- Latest version comes first (reverse chronological)
- Each version header links to a GitHub diff via footnote-style links at the bottom
- Date format: ISO 8601 (`YYYY-MM-DD`)
- Group same types of changes together
- Write for humans, not machines — no commit log dumps

### When to Update CHANGELOG

**Every PR** — add entry under `[Unreleased]` in the same PR as the code change. Do NOT postpone to release time.

**At release time** — rename `[Unreleased]` to `[0.x.y] - YYYY-MM-DD`, add new empty `[Unreleased]`, update comparison links at bottom.

**NEVER forget** — if committing code changes without a CHANGELOG entry, stop and add one first.

## Release Checklist

1. Determine version bump type (minor or patch) based on changes since last release
2. Move `[Unreleased]` entries in `CHANGELOG.md` to new version section with date
3. Add new empty `[Unreleased]` section
4. Update comparison links at bottom of `CHANGELOG.md`
5. Update `version` in `package.json` to new version
6. Commit version bump + changelog update
7. Create git tag: `git tag v0.X.Y`
8. Push tag: `git push origin v0.X.Y`
9. Create GitHub release: `gh release create v0.X.Y -t "v0.X.Y — Short description" -n "Release notes"`
10. Mark latest release with `--latest` flag

## Rules

- **Never** use `1.x.y` or `2.x.y` — project is beta (`0.x.y`)
- **Never** skip minor versions (go sequentially: 15 → 16 → 17)
- Patch versions reset to 0 on each minor bump
- Breaking changes are allowed in any `0.x.0` release (SemVer spec)
- Tag format always includes `v` prefix: `v0.X.Y`
- Release title format: `v0.X.Y — Short description of changes`

## Current State

- Latest: v0.20.1
- Next minor: v0.21.0
- Next patch: v0.20.2
