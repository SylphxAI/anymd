---
name: release
description: Publish new version and monitor release process
agent: coder
---

# Release & Publish

Prepare, publish, and monitor package release.

## Pre-Release Checks

**Quality Gates:**
- [ ] All tests pass
- [ ] No lint errors
- [ ] Build successful
- [ ] No security vulnerabilities
- [ ] Dependencies up to date
- [ ] CHANGELOG updated
- [ ] README accurate
- [ ] Breaking changes documented

**Version Decision:**
- Breaking changes → `major`
- New features → `minor` (default)
- Bug fixes → `patch`

## Release Process

anymd releases from `main` only: a version bump pull request is the release.

1. **Bump the version in every manifest:**
   ```bash
   bun scripts/set-version.ts X.Y.Z
   cargo update -w
   ```
   Add a `## X.Y.Z` section to `CHANGELOG.md`.

2. **Open one pull request** with the bump, the lockfile, and the changelog.
   Merging it runs `release.yml`, which publishes `@sylphx/anymd`, the
   platform binaries, and the `@sylphx/citra` and `@sylphx/pdf-reader-mcp`
   aliases.

3. **Monitor CI:**
   ```bash
   gh api 'repos/SylphxAI/anymd/actions/workflows/release.yml/runs?per_page=5'
   ```

4. **Verify Publication:**
   ```bash
   npx -y @sylphx/anymd@X.Y.Z version
   ```

## Post-Release

- [ ] Verify package published
- [ ] Test installation: `npx -y @sylphx/anymd@latest version`
- [ ] Create GitHub release with notes
- [ ] Announce (if public package)
- [ ] Close related issues/PRs

## Troubleshooting

**CI fails on install:**
- Update lockfile locally: `bun install`
- Commit and push

**Tests fail in CI:**
- Run tests locally: `bun run test:rust`, then `bun run build` and `bun run test:cov`
- Fix issues, commit, push

**Build fails:**
- Check build locally: `bun run build`
- Fix errors, commit, push

## Exit Criteria

- [ ] Package published successfully
- [ ] CI workflow completed
- [ ] GitHub release created
- [ ] Version verified on registry
- [ ] Installation tested

Report: Version number, publish time, registry URL.
