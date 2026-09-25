# Security Policy

## Supported versions

Security fixes land on the latest published `@sylphx/anymd` release. We
fix forward — please upgrade to the newest version before reporting.

## Reporting a vulnerability

**Please report privately. Do not open a public issue for security defects.**

Preferred channel — GitHub private vulnerability reporting (does not depend on
email delivery):

1. Go to the repository's **Security** tab.
2. Click **Report a vulnerability** (GitHub private advisory).
3. Include the details below.

This routes straight to the maintainers and lets us collaborate on a fix and a
coordinated advisory in one place.

Please include:

- affected version(s) and transport (`stdio` or `http`);
- a clear description and impact;
- minimal steps to reproduce or a proof of concept;
- any suggested remediation.

We aim to acknowledge a report within a few days, agree on severity and a fix
window, and publish a GitHub Security Advisory (GHSA, with a CVE where it
qualifies) once a fixed version ships. We are glad to credit reporters by their
chosen handle.

## Scope

In scope: defects in this package's own code — authentication/authorization
bypass, information disclosure, path traversal, SSRF, and similar. Issues in
third-party dependencies should be reported upstream; tell us if this package's
configuration meaningfully amplifies them.

## Operator hardening checklist (HTTP transport)

The HTTP transport (`MCP_TRANSPORT=http`) exposes every PDF tool to any client
that can reach the port. It is opt-in; the default transport is `stdio`.

- Binds to `127.0.0.1` (loopback) by default. Set `MCP_HTTP_HOST` to a
  non-loopback host only when you intend remote access.
- Set `MCP_API_KEY` whenever the server is reachable from another machine.
  Every `/mcp` request must then present a matching `X-API-Key` header;
  unauthenticated requests get `401`. The server refuses a non-loopback bind
  without a key. `MCP_ALLOW_UNAUTHENTICATED_REMOTE=true` is an explicit
  break-glass override for isolated trusted networks.
- Restrict filesystem reach with `--allow-dir=<path>` (repeatable) or
  `MCP_PDF_ALLOWED_DIRS`. The environment value uses the operating system path
  separator (`:` on Unix, `;` on Windows); commas are also accepted. Relative
  paths resolve from the server startup directory, and symlinks are resolved
  before component-wise containment checks. Without an allowlist the process
  can read any PDF it has OS permission to open.
- URL sources resolving to private/loopback addresses are blocked by default
  (SSRF guard). Set `MCP_PDF_ALLOW_PRIVATE_IPS=true` only in a trusted network.
