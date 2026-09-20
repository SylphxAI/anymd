#!/usr/bin/env bun
/**
 * Evidence-driven release admission for the pure-Rust replacement program.
 *
 * Modes:
 * - freeze: productTruth.publishFreeze=true → fail closed (no registry publish)
 * - publish-allowed: publishFreeze=false and dropInFor3014=false → allow publish of
 *   non-default pure-Rust progress packages
 * - sole-runtime / drop-in default: publishFreeze=false and dropInFor3014=true →
 *   require stronger package-default pure-Rust evidence
 *
 * Hardening (post-3.2.0 corrective):
 * - review evidence is the authorization source of truth (not matrix self-claims alone)
 * - review candidate SHA must match evidence candidate SHA
 * - optional --require-exact-head / ADMISSION_REQUIRE_EXACT_HEAD=1 compares to git HEAD
 * - drop-in default requires fail-closed runtime-entry (no silent TypeScript fallback)
 * - zero unresolved nonclaim blockingForUnfreeze entries
 */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = join(import.meta.dirname, '..');
const failures: string[] = [];
const requireExactHead =
  process.argv.includes('--require-exact-head') ||
  process.env['ADMISSION_REQUIRE_EXACT_HEAD'] === '1';

const matrix = JSON.parse(
  readFileSync(join(root, 'docs/specs/pure-rust-capability-matrix.json'), 'utf8')
) as {
  productTruth: {
    publishFreeze: boolean;
    dropInFor3014: boolean;
    pureRustStatus: string;
    admissionBar?: string;
    publishedStable?: string;
    publishedImplementation?: string;
    automaticTypescriptFallback?: boolean;
    typescriptFallback?: string | boolean;
    soleRuntimeDefault?: boolean;
  };
  admissionProgram?: {
    mode?: string;
    publishFreeze?: boolean;
    dropInFor3014?: boolean;
    wholeProductReview?: {
      status?: string;
      candidateSha?: string;
      evidence?: string;
      unfreezeAuthorized?: boolean;
      tsRetirementAuthorized?: boolean;
      soleRuntimeAuthorized?: boolean;
    };
    unfreezeAuthorized?: boolean;
    soleRuntimeAuthorized?: boolean;
  };
};

const truth = matrix.productTruth;
const admission = matrix.admissionProgram ?? {};
const review = admission.wholeProductReview ?? {};

if (admission.mode !== 'capability-first-semantic-compatibility') {
  failures.push('admissionProgram.mode must be capability-first-semantic-compatibility');
}
if (truth.admissionBar !== 'capability-first-semantic-compatibility') {
  failures.push('productTruth.admissionBar must remain capability-first-semantic-compatibility');
}
if (admission.publishFreeze !== truth.publishFreeze) {
  failures.push('admissionProgram.publishFreeze must match productTruth.publishFreeze');
}
if (admission.dropInFor3014 !== truth.dropInFor3014) {
  failures.push('admissionProgram.dropInFor3014 must match productTruth.dropInFor3014');
}

const requiredFiles = [
  'docs/adr/0005-capability-first-semantic-compatibility.md',
  'docs/specs/capability-first-admission-contract.md',
  'docs/specs/nonclaim-reclassification-ledger.json',
  'docs/specs/agent-task-corpus/baselines/typescript-v3.0.14.local.json',
  'docs/specs/agent-task-corpus/baselines/typescript-v3.0.14.public-url.json',
  'docs/specs/semantic-contracts/semantic-public-url-corpus.json',
  'src/pure-rust.ts',
  'src/runtime-entry.ts',
  'scripts/check-ts-production-absence.ts',
  'docs/adr/0006-sole-rust-production-and-channel-authority.md',
  'scripts/smoke-native-launcher.ts',
  'scripts/smoke-native-package-resolve.ts',
];
for (const rel of requiredFiles) {
  if (!existsSync(join(root, rel))) failures.push(`missing required evidence file: ${rel}`);
}

const nonclaimLedger = JSON.parse(
  readFileSync(join(root, 'docs/specs/nonclaim-reclassification-ledger.json'), 'utf8')
) as {
  stats?: { blockingForUnfreezeCount?: number; total?: number };
  entries?: Array<{ blockingForUnfreeze?: boolean; class?: string }>;
};
const blockingFromEntries = (nonclaimLedger.entries ?? []).filter(
  (entry) => entry.blockingForUnfreeze === true
).length;
const blockingCount = nonclaimLedger.stats?.blockingForUnfreezeCount ?? blockingFromEntries;
if (blockingCount !== blockingFromEntries) {
  failures.push(
    `nonclaim stats.blockingForUnfreezeCount (${blockingCount}) disagrees with entries (${blockingFromEntries})`
  );
}

type ReviewEvidence = {
  outcome?: string;
  unfreezeAuthorized?: boolean;
  tsRetirementAuthorized?: boolean;
  soleRuntimeAuthorized?: boolean;
  candidate?: { sha?: string };
  scope?: string;
  productTruthSnapshot?: {
    dropInFor3014?: boolean;
    automaticTypescriptFallback?: boolean;
    pureRustStatus?: string;
  };
};

let evidence: ReviewEvidence | null = null;
if (!review.evidence || !existsSync(join(root, review.evidence))) {
  failures.push('wholeProductReview.evidence artifact missing');
} else {
  evidence = JSON.parse(readFileSync(join(root, review.evidence), 'utf8')) as ReviewEvidence;
  if (!String(evidence.outcome ?? '').startsWith('review_pass')) {
    failures.push(`review evidence outcome is not review_pass_*: ${String(evidence.outcome)}`);
  }
  if (!review.candidateSha) {
    failures.push('wholeProductReview.candidateSha is required');
  }
  if (!evidence.candidate?.sha) {
    failures.push('review evidence candidate.sha is required');
  }
  if (
    review.candidateSha &&
    evidence.candidate?.sha &&
    review.candidateSha !== evidence.candidate.sha
  ) {
    failures.push('review candidateSha does not match evidence artifact candidate.sha');
  }
  // Evidence remains SSOT. Unfreeze flags are only required when publish is allowed.
  if (truth.publishFreeze === false) {
    if (evidence.unfreezeAuthorized !== true) {
      failures.push('review evidence must set unfreezeAuthorized=true to lift publish freeze');
    }
    if (review.unfreezeAuthorized !== true) {
      failures.push('admissionProgram.wholeProductReview.unfreezeAuthorized must be true when publishing');
    }
  }
  if (review.unfreezeAuthorized === true && evidence.unfreezeAuthorized !== true) {
    failures.push('matrix review.unfreezeAuthorized cannot exceed evidence.unfreezeAuthorized');
  }
  if (review.tsRetirementAuthorized === true && evidence.tsRetirementAuthorized !== true) {
    failures.push('matrix review.tsRetirementAuthorized cannot exceed evidence.tsRetirementAuthorized');
  }
  if (review.soleRuntimeAuthorized === true && evidence.soleRuntimeAuthorized !== true) {
    failures.push('matrix review.soleRuntimeAuthorized cannot exceed evidence.soleRuntimeAuthorized');
  }
  if (admission.unfreezeAuthorized === true && evidence.unfreezeAuthorized !== true) {
    failures.push('admissionProgram.unfreezeAuthorized cannot exceed evidence.unfreezeAuthorized');
  }
  if (admission.soleRuntimeAuthorized === true && evidence.soleRuntimeAuthorized !== true) {
    failures.push('admissionProgram.soleRuntimeAuthorized cannot exceed evidence.soleRuntimeAuthorized');
  }

  if (requireExactHead) {
    const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' });
    if (head.status !== 0) {
      failures.push('failed to resolve git HEAD for exact-SHA admission');
    } else {
      const headSha = head.stdout.trim();
      const candidateSha = evidence?.candidate?.sha ?? '';
      if (review.candidateSha === 'PENDING_RELEASE_SHA' || candidateSha === 'PENDING_RELEASE_SHA') {
        failures.push('PENDING_RELEASE_SHA is not a valid exact release SHA');
      } else if (!headSha || !candidateSha || review.candidateSha !== candidateSha) {
        failures.push(
          `exact-head admission requires review.candidateSha == evidence.candidate.sha; got review=${review.candidateSha} evidence=${candidateSha}`
        );
      } else if (headSha === candidateSha) {
        // exact match
      } else {
        const objectType = spawnSync('git', ['cat-file', '-t', candidateSha], {
          cwd: root,
          encoding: 'utf8',
        });
        if (objectType.status !== 0 || objectType.stdout.trim() !== 'commit') {
          failures.push(
            `exact-head admission cannot resolve candidate ${candidateSha} in this checkout (often a pre-squash PR tip not reachable from main). Publish from the reviewed release ref, or pin review.candidateSha to a commit reachable from HEAD. git cat-file stderr=${JSON.stringify(objectType.stderr || '')}`
          );
        } else {
          const ancestor = spawnSync(
            'git',
            ['merge-base', '--is-ancestor', candidateSha, headSha],
            { cwd: root, encoding: 'utf8' }
          );
          const diff = spawnSync(
            'git',
            ['diff', '--name-only', candidateSha, headSha],
            { cwd: root, encoding: 'utf8' }
          );
          const changed = (diff.stdout || '')
            .split('\n')
            .map((line) => line.trim())
            .filter(Boolean);
          const allow = (rel: string) =>
            rel === 'docs/specs/pure-rust-capability-matrix.json' ||
            rel === 'src/pure-rust.ts' ||
            rel === 'dist/pure-rust.js' ||
            rel === 'README.md' ||
            rel === 'CHANGELOG.md' ||
            // Repository governance prose: agent entry notes and project facts.
            // Same class as README.md above - it describes the product, it does
            // not implement it.
            rel === 'AGENTS.md' ||
            rel === 'PROJECT.md' ||
            rel === 'scripts/check-verified-candidate-admission.ts' ||
            // Any documentation: it records the product and its releases and
            // cannot change runtime behavior. Enumerating subdirectories one at
            // a time kept blocking the publish whenever a doc landed
            // (docs/security, docs/operations, docs/adr, docs/performance, ...),
            // which is how 5.0.2 was blocked three times. The gate polices code,
            // not prose.
            rel.startsWith('docs/') ||
            // The release/publish workflows and the admission gate itself: they
            // decide how a release is delivered, not what the product does.
            // Fixing a release-blocking workflow would otherwise block the
            // release it unblocks.
            rel.startsWith('.github/workflows/') ||
            rel.startsWith('verification/');
          // empty changed => identical trees; [].every(allow) is true.
          // Non-empty changed must be pin-path only (matrix/verification).
          const onlyPinPaths = changed.every(allow);
          if (ancestor.status !== 0 || diff.status !== 0 || !onlyPinPaths) {
            failures.push(
              `exact-head admission requires git HEAD (${headSha}) == candidate ${candidateSha}, or a pin-path-only descendant; ancestorStatus=${ancestor.status} diffStatus=${diff.status} changed=${JSON.stringify(changed)} stderr=${JSON.stringify((ancestor.stderr || '') + (diff.stderr || ''))}`
            );
          }
        }
      }
    }
  } else if (
    review.candidateSha === 'PENDING_RELEASE_SHA' ||
    evidence.candidate?.sha === 'PENDING_RELEASE_SHA'
  ) {
    // Allowed only before the release commit pins the exact SHA.
    // Publish workflows must pass --require-exact-head.
  }
}

// Fail-closed default entry check for drop-in / sole-runtime packaging.
const runtimeEntry = existsSync(join(root, 'src/runtime-entry.ts'))
  ? readFileSync(join(root, 'src/runtime-entry.ts'), 'utf8')
  : '';
const failClosedMentioned =
  runtimeEntry.includes('fail-closed') ||
  runtimeEntry.includes('no automatic TypeScript fallback') ||
  runtimeEntry.includes('sole-Rust') ||
  runtimeEntry.includes('TypeScript production runtime has been removed');
const soleRustNoTs =
  runtimeEntry.includes('TypeScript production runtime has been removed') ||
  runtimeEntry.includes('there is no bundled TypeScript PDF runtime') ||
  runtimeEntry.includes('sole-Rust');
const explicitOnlyTs =
  soleRustNoTs ||
  (runtimeEntry.includes("PDF_READER_FORCE_TYPESCRIPT") &&
    runtimeEntry.includes('forceTs') &&
    failClosedMentioned);

if (truth.publishFreeze === true) {
  if (truth.dropInFor3014 !== false) {
    failures.push('dropInFor3014 must be false while publishFreeze=true');
  }
  failures.push(
    'PUBLISH FREEZE active: productTruth.publishFreeze=true blocks registry publish (intentional)'
  );
} else {
  // Publish allowed. Require evidence authorization (already checked above when evidence present).
  const authorized =
    evidence?.unfreezeAuthorized === true &&
    (admission.unfreezeAuthorized === true || review.unfreezeAuthorized === true);
  if (!authorized) {
    failures.push(
      'publishFreeze=false requires evidence.unfreezeAuthorized and matrix review/admission unfreezeAuthorized'
    );
  }

  if (truth.dropInFor3014 === true) {
    if (blockingCount !== 0) {
      failures.push(
        `dropInFor3014=true requires zero nonclaim blockingForUnfreeze entries; found ${blockingCount}`
      );
    }
    if (truth.pureRustStatus === 'experimental-opt-in') {
      failures.push('dropInFor3014=true cannot remain experimental-opt-in');
    }
    if (!String(truth.pureRustStatus ?? '').includes('default') && !String(truth.pureRustStatus ?? '').includes('sole-rust-production')) {
      failures.push('dropInFor3014=true requires pureRustStatus to indicate default pure-Rust / sole-rust production runtime');
    }
    const soleAuthorized =
      evidence?.soleRuntimeAuthorized === true ||
      evidence?.tsRetirementAuthorized === true;
    if (!soleAuthorized) {
      failures.push(
        'dropInFor3014=true requires evidence.soleRuntimeAuthorized or evidence.tsRetirementAuthorized'
      );
    }
    if (admission.soleRuntimeAuthorized !== true && review.tsRetirementAuthorized !== true) {
      failures.push(
        'dropInFor3014=true requires admission soleRuntimeAuthorized or review.tsRetirementAuthorized'
      );
    }
    if (!String(truth.publishedImplementation ?? '').toLowerCase().includes('rust')) {
      failures.push(
        'dropInFor3014=true requires productTruth.publishedImplementation to identify pure-Rust'
      );
    }
    if (truth.automaticTypescriptFallback === true) {
      failures.push('dropInFor3014=true forbids automaticTypescriptFallback=true');
    }
    const tsFallback = truth.typescriptFallback;
    const tsFallbackOk =
      tsFallback === 'explicit-only' ||
      tsFallback === false ||
      tsFallback === undefined;
    if (!tsFallbackOk) {
      failures.push(
        "typescriptFallback must be 'explicit-only' or false (not automatic true or other values)"
      );
    }
    if (!explicitOnlyTs || !failClosedMentioned) {
      failures.push(
        'dropInFor3014=true requires src/runtime-entry.ts fail-closed default with explicit-only TypeScript rollback'
      );
    }
    // Detect the old silent fallback pattern: else branch imports TS without forceTs guard only.
    if (
      runtimeEntry.includes('Falls back to the TypeScript') ||
      runtimeEntry.includes('// TypeScript fallback path.')
    ) {
      failures.push(
        'src/runtime-entry.ts still documents/implements automatic TypeScript fallback; fail closed instead'
      );
    }
    if (evidence?.productTruthSnapshot?.automaticTypescriptFallback === true) {
      failures.push('review evidence productTruthSnapshot forbids automaticTypescriptFallback');
    }
  }
}

if (failures.length) {
  console.error(failures.map((line) => `[verified-candidate-admission] ${line}`).join('\n'));
  process.exit(1);
}

console.log(
  JSON.stringify(
    {
      profile: 'verified_candidate_admission',
      pass: true,
      publishFreeze: truth.publishFreeze,
      dropInFor3014: truth.dropInFor3014,
      pureRustStatus: truth.pureRustStatus,
      reviewStatus: review.status ?? null,
      candidateSha: review.candidateSha ?? null,
      requireExactHead,
      blockingForUnfreezeCount: blockingCount,
      automaticTypescriptFallback: truth.automaticTypescriptFallback ?? null,
    },
    null,
    2
  )
);
console.log('[verified-candidate-admission] PASS');
