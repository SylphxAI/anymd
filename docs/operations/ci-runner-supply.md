# Why this repository's CI waits while other repositories run

Observed 2026-09-20. Read this before concluding "CI is broken" or "the runners
are down" — for this repository those are both wrong, and the real shape is a
runner-group membership fact, not a fleet outage.

## The shape

A GitHub Actions job runs only on a runner in a group that is allowed to serve
the repository. Organization runner groups with `visibility: selected` each name
an explicit repository list, and **a repository that is not in a group can never
use that group's runners** regardless of how many are online.

This repository is **public** (`SylphxAI/pdf-reader-mcp`, repo id `960549454`),
and:

| Group | Visibility | `allows_public_repositories` | Runners | Can serve this repo? |
| --- | --- | --- | --- | --- |
| `Default` (id 1) | `all` | **false** | 0 | **No** — public repos excluded |
| `sylphx-repo-960549454` (id 20) | `selected` | true | 4 | **Yes — this is the only route** |

Group 20's four runners were all `offline` through the observation window while
other repositories' dedicated groups had online runners and were executing jobs.

So the correct statement is: **this repository has exactly one runner group, and
that group's runners were offline.** Other repos draining their queues is not
evidence that this repo's queue will drain.

## Why the earlier reading was wrong

The org-wide count (`292 runners, all offline`) looked like a fleet outage and
was reported as one. It was measured with
`gh api orgs/SylphxAI/actions/runners` at a moment before the pool recovered, and
it hides the per-repository membership fact entirely: a repository's eligibility
depends on its group, not on the org total. The org total later recovered to
dozens online while this repository's jobs kept waiting, which is the tell that
the two facts are independent.

## How to diagnose a stuck queue for a repository

```bash
RID=$(gh api repos/OWNER/REPO --jq .id)
# which groups can serve this repo?
gh api /orgs/OWNER/actions/runner-groups --paginate \
  --jq ".runner_groups[] | \"\(.id) \(.name) \(.visibility) public=\(.allows_public_repositories)\""
# the repo's own group, and whether its runners are alive
GID=$(gh api /orgs/OWNER/actions/runner-groups --paginate \
  --jq ".runner_groups[] | select(.name==\"sylphx-repo-$RID\") | .id")
gh api "/orgs/OWNER/actions/runner-groups/$GID/runners" \
  --jq '.runners[] | "\(.name) \(.status) busy=\(.busy)"'
```

If the repository's group has zero online runners while other groups are busy,
the fix is runner supply **for that group**, not a fleet-wide action.

## Boundary

Runner group membership and per-group supply are provisioned by Hands
(`SylphxAI/hands` `selected_runner_group_worker` / `provider_mint`) and owned in
`SylphxAI/cloud`'s CI configuration — the same place that created
`sylphx-repo-960549454` as a per-repository group. This repository can observe
and report the state; it does not own the group.

## The rule this record adds

**A repository's CI capacity is a property of its runner group, not of the org
runner total.** Report the group and its online count, never the org count; an
org-wide "runners are down" reading can be false for a given repository and
true for its neighbours at the same time.


## Update 2026-09-20: what the intermittent outage actually is

The repository's group (`sylphx-repo-960549454`) went fully offline and back
several times over one day — 0 online for long stretches, then 37-75 online and
draining normally. Jobs queued through the outages (a fan-in job sat 71 minutes
and then completed successfully; a Release run waited ~3 hours and passed).
Nothing in this repository is wrong when that happens.

What was true on the cluster side during the offline windows, observed directly:

- Hands restarted (new pod hash at 16:34Z) and is healthy: `jit_width=431`,
  no crashloop, no restart count.
- But the only thing it logs is the periodic `jit_width` tick — no mint work is
  being driven, so no runner registers and the whole org's queue stands still
  (`cloud`, `apps`, `hands`, `agents`, `identity` all showed `in_progress=0`).
- During the working windows the same Hands mints and settles runner jobs, and
  the queue drains across every repository.

So the condition is **upstream runner supply in `SylphxAI/hands` /
`SylphxAI/cloud`**, not a property of this repository. Two consequences worth
recording:

1. **A queued job is not a failed job.** Every outage so far has drained: the
   jobs completed on their own once supply returned. Do not re-push, re-run, or
   loosen a gate because a queue looks stuck.
2. **The admission gate interacts badly with this.** A release-blocking doc or
   workflow change moves HEAD past the review pin, so the next publish needs a
   fresh pin — and each re-pin is itself a new run queued behind the outage. That
   is why `docs/**` and `.github/workflows/**` are now pin paths
   (`scripts/check-verified-candidate-admission.ts`): the gate should block on
   unreviewed code, not on the release's own paperwork.
