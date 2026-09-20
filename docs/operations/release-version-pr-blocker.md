# Why every Release run failed between 2026-09-17 and 2026-09-20

Read this before touching `.gitignore`'s staged-native block or renaming a
native binary. The mistake was a rename that left an ignore pattern stale, and
it silently broke the only path to npm for three days.

## What a maintainer saw

`Release` ran on every push to `main` and failed at its last step, `Create
release PR`. Every gate before it passed — Biome, typecheck, package build,
Rust MCP build, package smoke, coverage tests, docs, benchmarks, the SOTA
release gate, and verified-candidate admission. The failure text:

```
Error: Unexpected executable file at packages/citra-linux-x64-gnu/bin/citra-mcp-server,
GitHub API only supports non-executable files and directories.
You may need to add this file to .gitignore
```

`changesets/action` with `commitMode: github-api` refuses to create a commit
containing an executable file. A 17 MB, mode-`100755` host binary was tracked at
that path.

## Why it slipped

`.gitignore` has a "never commit host binaries" block, and it is correct in
intent. Its patterns named the **pre-rename** binary:

```
packages/**/bin/pdf-reader-mcp-server
```

When #626 renamed the binary to `citra-mcp-server`, `bin/native/**` stayed
covered by a broader pattern, but `packages/**/bin/citra-mcp-server` did not.
That left the one path the scaffold workflow actually stages unignored, so it
was committed.

The tell: the old pattern is now a name nothing produces. **An ignore rule that
matches no existing artifact is not harmless — it is a rule that stopped
working.**

## The fix

Ignore the renamed binary on both staged paths, keeping the pre-rename names so
a stale checkout stays covered. The binary is staged by the scaffold and publish
workflows, never committed.

## How to verify the fix

The version PR is what was failing, so exercise that exact step locally:

```bash
git checkout -B tmp/verify-version <main>
GITHUB_TOKEN=$(gh auth token) bun node_modules/@changesets/cli/bin.js version
bun run native:sync-manifests
bun run sync:server-json
# then: no file in the change set may be mode 100755
git status --porcelain | awk '{print $2}' | while read -r f; do
  [ -f "$f" ] || continue
  mode=$(git ls-files -s -- "$f" | awk '{print $1}')
  [ -n "$mode" ] && [ "$mode" != 100644 ] && echo "EXEC-CHANGED: $f ($mode)"
done
```

Verified 2026-09-20: the script completes, bumps to the next patch, syncs every
native manifest and `server.json`, and reports no executable file in the change
set.

## The rule this record adds

A renamed artifact must carry its ignore rule, its staging paths, and its
assertions with it. When you rename a binary, a package, or a generated output,
grep the patterns and the workflows for the old name in the same change; a
pattern that no longer matches anything is a silently disabled guard.

## Related

The fleet-wide CI queue stall observed in the same window is separate: it is a
runner-supply outage, not this defect. This record covers only the version-PR
blocker, which was reproducible on `main` regardless of runner state.
