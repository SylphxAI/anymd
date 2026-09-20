# npm provenance cannot be attested from a self-hosted runner

Observed 2026-09-20 on the first publish attempt for `5.0.2`.

## What happened

All five platform builds succeeded. The publish step failed on the first package:

```
npm error code E422
npm error 422 Unprocessable Entity - PUT https://registry.npmjs.org/@sylphx%2fcitra-darwin-arm64
npm error - Error verifying sigstore provenance bundle:
  Unsupported GitHub Actions runner environment: "self-hosted".
  Only "github-hosted" runners are supported when publishing with provenance.
```

`release-publish-with-natives.ts` passed `--provenance` unconditionally, and
every runner this product builds on is self-hosted (`sylphx-linux-standard`, and
the self-hosted macOS pool). npm cannot produce a sigstore attestation there, so
the request is rejected **before** any package is accepted.

## Why it went unnoticed

Nothing else in the pipeline exercises the publish path. CI, Release, the
differential harness, and the admission gate all pass without ever calling
`npm publish`, so a release could be blocked indefinitely while every visible
check stayed green. The failure only appears on the one step that mutates the
registry.

## The fix

Request provenance only where npm can actually attest it:

| Context | `--provenance` |
| --- | --- |
| GitHub-hosted Actions runner | **yes** — npm can attest |
| Self-hosted Actions runner (this fleet) | no — npm returns E422 |
| No Actions context (maintainer shell) | no — nothing to attest |

`CITRA_NPM_PROVENANCE=1|0` overrides the decision for a deliberate run. When the
flag is omitted the publish logs that it is publishing **without** a provenance
bundle, so the record never claims an attestation that does not exist.

## The rule this record adds

**A platform limit is not a product defect, and hiding it is worse than naming
it.** npm provenance requires a GitHub-hosted runner; this fleet is self-hosted
by design (registered runners, no GitHub-hosted macOS). The honest options are
to publish without the attestation and say so, or to move the publish step to a
hosted runner. Silently dropping `--provenance`, or leaving it in so every
release fails, are both worse: one overstates supply-chain evidence, the other
blocks customers on a guarantee that this fleet cannot meet.

If an attestation is required for customer trust, the publish job — not the
build fleet — is the thing to move.
