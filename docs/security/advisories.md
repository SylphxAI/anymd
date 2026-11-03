# Published advisories and their CVE IDs

Last read 2026-10-20 from `GET /repos/SylphxAI/anymd/security-advisories`.

Every advisory we publish gets a CVE ID, because dependency scanners key on the
CVE and not on the GHSA id. GitHub is the CNA for this repository, so the id is
requested from the advisory page ("Request CVE") or with
`POST /repos/SylphxAI/anymd/security-advisories/{ghsa_id}/cve`, and GitHub
assigns it. GitHub usually answers within 72 hours, and the id only shows on the
advisory once its advisory is public.

## One vulnerability, one advisory

A report that turns out to be the same defect as an already published advisory
stays in the repository in one of two states, never as a second published record:

- **closed** — an advisory that was still in triage or draft. Closing is the
  normal path and the URL keeps resolving with its description intact.
- **published** — a report that was published before the duplication was known.
  GitHub does not close a published advisory through the UI or the API (`422
  Cannot close published advisory`); the only removal is a request to GitHub
  Support, so the record stays until that request is granted.

## The pdf_compare allowlist bypass

`GHSA-xrc8-gj3q-m8vm` is the canonical record: `@sylphx/anymd >=7.0.0 <=8.5.4`
and `@sylphx/citra >=5.0.4 <=8.5.4`, patched in 8.5.5.

`GHSA-f8xf-6vpc-6hpj` describes the same defect with narrower ranges (from
8.0.0). It was published before the duplication was seen, so it is still
published; its summary and description say it is the duplicate and name
`GHSA-xrc8-gj3q-m8vm` as the record to follow.

## Current state

| Advisory | Product | Canonical record |
| --- | --- | --- |
| GHSA-xrc8-gj3q-m8vm | `pdf_compare` allowlist bypass | this one |
| GHSA-5r2f-7788-qp8v | DNS-rebinding SSRF, TS-and-Rust lines | this one |
| GHSA-rgg9-pwc3-jg39 | DNS-rebinding SSRF, TypeScript engine | this one |
| GHSA-qr3f-g5wf-225w | `--allow-dir` not implemented in the Rust server | this one |
| GHSA-f3xw-ff5r-rj7c | SSRF via IPv6 transition addresses | this one |
| GHSA-q344-5v34-gm84 | HTTP transport ignored `MCP_API_KEY` | this one |
| GHSA-34gp-w56h-r2mv | `read_pdf` url `file://` confinement bypass | this one |
| GHSA-f8xf-6vpc-6hpj | duplicate of GHSA-xrc8-gj3q-m8vm | GHSA-xrc8-gj3q-m8vm |

CVE IDs are requested for every row that does not yet carry one. Track the
request rather than re-filing it: a second request for the same advisory is
answered with `422`.
