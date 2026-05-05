---
description: Format auto-generated indexer release notes into structured MNF-aligned release notes
---

Format release notes for a midnight-indexer release. The user provides $ARGUMENTS — a release tag (e.g. `v4.3.0`), bare version (`4.3.0`), or GitHub release URL. Modelled on the node skill at `midnightntwrk/midnight-node/.claude/skills/format-release-notes.md`; differences captured at the bottom.

## 1. Normalize Input

- If `$ARGUMENTS` is a URL, extract the tag (last path segment).
- If the tag does not start with `v`, prepend `v`. Store as `TAG`. Bare version is `VERSION`.
- RC if tag contains `-rc.`.
- Determine the **release line**: 4.0.x = maintenance line on node 0.22.x; 4.x.y where x ≥ 1 = development line on node 1.0+. Affects step 5 (Release type / Dependencies).

## 2. Fetch This Release

```bash
gh release view TAG --repo midnightntwrk/midnight-indexer --json body,publishedAt,tagName
git rev-parse "$TAG^{tree}"
```

If the tag isn't local, `git fetch origin tag $TAG --no-tags` first.

## 3. Fetch Prior Releases for Deduplication

`gh release list --repo midnightntwrk/midnight-indexer --limit 50 --json tagName` (in parallel with step 2). Identify priors:

- **RC** (e.g. `v4.3.0-rc.3`): collect all earlier RCs for the same version. Each RC's notes only contain its own delta.
- **Final, dev line** (e.g. `v4.3.0`): prior is the previous final on the same line. Skip RCs.
- **Final, maintenance line** (e.g. `v4.0.2`): prior is `v4.0.1`. Skip RCs and dev-line releases.

Fetch all prior bodies in parallel via `gh release view PRIOR_TAG --json body`. Extract PR numbers (pattern `#\d+`, `/pull/\d+`) into an exclude set.

## 4. Parse + Classify

Parse the matching version section from `CHANGELOG.md` (git-cliff produces it from conv-commits):

```bash
awk '/^## \['"$VERSION"'\]/,/^## \[/' CHANGELOG.md | sed '$d'
```

git-cliff sections: `### 🚀 Features`, `### 🐛 Bug Fixes`, `### ⚡ Performance`, `### 🚜 Refactor`, `### 📚 Documentation`, `### ⚙️ Miscellaneous Tasks`. Fallback to GH release body if CHANGELOG missing. Remove entries whose PRs are in the prior-set.

Classify by conv-commit prefix:

| Prefix | Section |
|--------|---------|
| `feat:` | New Features |
| `feat!:` or `BREAKING CHANGE:` footer | Breaking Changes or Required Actions |
| `fix:` | Fixed Defects |
| `perf:`, `refactor:` | Improvements |
| `chore(deps):` | Dependencies (omit unless CVE patch) |
| `chore:`, `chore(ci):`, `chore(release):`, `docs:`, `test:`, `style:` | Skip from formal RN |

Indexer is a single deployable (no runtime/node/toolkit split), so every change ships in one image set. If a classification is ambiguous, ask in step 6.

## 5. Determine Release Type and Dependencies

Added 1 May 2026 after MNF template alignment with Thiago Earp and Giles Cope. Populate from the release line in step 1:

- 4.0.x: `Maintenance backport (node 0.22.x compatible)`
- 4.x.y RC (x ≥ 1): `Pre-release (development line)`
- 4.x.y final (x ≥ 1), part of a Midnight bundle: `Bundle component — Midnight X.Y`
- 4.x.y final (x ≥ 1), standalone: `Patch / Minor release`

When in doubt, ask the user.

**Dependencies** lists paired components:
- Maintenance line: paired node 0.22.x tag, toolkit/ledger versions tested with.
- Bundle component: link to the Midnight Release X.Y tracking issue (e.g. `https://github.com/midnightntwrk/midnight-engineering/issues/1` for 1.1) and bundle siblings.
- Standalone: cross-repo bumps from changelog (ledger 8.1.x, node 1.0.0-rc.N, etc.).

## 6. Fetch Known Issues + Present for Review

**RC**: skip Known Issues entirely. Only present the classification table for confirmation.

**Final**:

```bash
gh issue list --repo midnightntwrk/midnight-indexer \
  --label "priority:critical,priority:high" \
  --state open --json number,title,url,labels
```

Exclude any issue numbers fixed in this release. Indexer moved off Jira so GH issues are canonical.

Present in a single prompt: classification table + Release type/Dependencies values + filtered Known Issues + (if dev-line and maintenance line is active) a draft compatibility note for the top of the body. User confirms in one round.

## 7. Generate Output

Write to `release-notes-VERSION.md` in repo root. No markdownlint pass (Giles confirmed 21 Apr that MNF renders downstream).

### Template

1. `# Midnight Indexer VERSION Release Notes`
2. Metadata block (each on its own line, blank line above):
   - `**Release date:** YYYY-MM-DD`
   - `**Release type:** <from step 5>`
   - `**Git tag:** [TAG link]`
   - `**Tree hash:** <git rev-parse output>`
   - `**Environment:** All public networks (mainnet, preprod, preview, devnet, qanet)` (adjust for maintenance-line scope)
3. **(Dev line + maintenance line both active)** — Compatibility note as a top-of-body blockquote, e.g. "> Note: This release pairs with node 1.0.0 and ships as part of the May release bundle. Refer to MNF advisories for current deployment recommendations."
4. `## Dependencies` — per step 5
5. `## Docker Images` — list per what's actually built at this release tag. The image set has changed over time; check `.github/workflows/build-indexer-images.yaml` at the release tag to confirm which images publish. Reference points:
   - **4 images** for v4.0.x maintenance backports (no spo-indexer yet): chain-indexer, indexer-api, wallet-indexer, indexer-standalone
   - **5 images** for current development line (with spo-indexer): chain-indexer, indexer-api, wallet-indexer, indexer-standalone, spo-indexer
   - Format each as `midnightntwrk/<name>:VERSION`. Annotate any image-count change in the release notes (e.g. "spo-indexer image is new in this release").
6. `## Summary` — 1-3 sentences
7. `## Audience` — checklist (operators on public networks, testnet admins, DApp devs against the API, QA/release managers)
8. `## What Changed` — table (Change | Type | PR), separator `| --- | --- | --- |`
9. `## New Features` — from `feat:` commits, with operator/developer notes
10. `## Improvements` — bullets from `perf:` / `refactor:`
11. `## Deprecations` — omit if empty
12. `## Breaking Changes or Required Actions` — from `feat!:` / `BREAKING CHANGE:`. If empty, write "None." (auditors want this affirmatively)
13. `## Known Issues` — Description / Issue link / Workaround. Omit for RCs.
14. `## Fixed Defects` — table (Defect / PR | Description), separator `| --- | --- |`
15. `## Links and References` — all PR/issue links as markdown. Plus full changelog, schema, GH release URLs.
16. `## Full Change Details` — verbatim CHANGELOG.md section for this version.

Omit empty sections except Breaking Changes (always include affirmatively).

## 8. Offer to Update GitHub Release

After writing, ask before running:

```bash
gh release edit TAG --repo midnightntwrk/midnight-indexer --notes-file release-notes-VERSION.md
```

Never run without explicit confirmation.

## Notes

- No LLM watermarks or co-authored-by lines.
- Indexer is a single deployable, no runtime/node/toolkit split.
- Release line distinction (4.0.x maintenance vs 4.x.y dev) drives Release type, Dependencies, and the optional compatibility note.

## Differences from node skill

| Aspect | Node | Indexer |
|--------|------|---------|
| Tag prefix | `node-X.Y.Z` | `vX.Y.Z` |
| Input | Per-component change files | git-cliff CHANGELOG (conv-commits) |
| Classification | Tag-based (`#runtime`, etc.) | Conv-commit prefix |
| Runtime/node distinction | Critical | N/A |
| Images | 2 (node, toolkit) | 5 (chain-indexer, indexer-api, wallet-indexer, indexer-standalone, spo-indexer) |
| Known Issues | JIRA (Highest/Blocker) | GH issues (priority:critical/high) |
| Markdownlint | Required | Skipped |
| Release type / Dependencies / Compatibility note | Not in node skill yet | Indexer-specific (1 May 2026) |

The Release type / Dependencies / Compatibility note fields are indexer-specific responses to parallel-release-line situations (4.0.x maintenance + 4.x.y dev). May be added to the node skill on Giles's next iteration; re-align this skill at that point. Tracked in `docs/interactions/release-notes-thiago/thiago-discussions-full-arc.md`.
