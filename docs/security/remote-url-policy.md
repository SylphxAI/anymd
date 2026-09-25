# Remote URL policy — anymd

anymd accepts a `url` source because many agents are handed a PDF link rather
than a path. Remote URLs are in scope, but only as the narrow fetch described
here.

## What a `url` source is for

- One http(s) PDF body, fetched by the local process and treated exactly like a
  local PDF after download.
- An explicit convenience for a caller that already has a link.

It is not a browser, a crawler, a general web client, an authentication
delegate, or a way to reach a private network. Agents that need to research the
web should use Lookout.

## What the fetch does

- `http` and `https` only; every other scheme is rejected.
- The hostname is resolved locally and the resolved address is pinned for the
  connection: the address the guard checked is the address the socket connects
  to. A fresh, pool-free client makes every hop a separate validation and
  connection boundary.
- Redirects are followed only after the target is resolved and re-validated, up
  to five hops.
- Non-public addresses are rejected by default.
- `MCP_PDF_ALLOW_PRIVATE_IPS=true` is an explicit opt-in for local fixtures or a
  trusted network; it is never the default.
- A 30-second timeout, a 256 MiB body limit, no environment proxy, and no
  `file://` access.
- The downloaded body is written to a secure temporary file and removed after
  use.

## Evidence

The guarantee is covered by the Rust tests in
`crates/pdf-reader-core/src/url_fetch.rs`, including
`one_dns_resolution_is_pinned_to_the_actual_connection` and the redirect
re-validation tests. The published artifact is the pure-Rust engine; residual
TypeScript URL code is an oracle and not production authority.

## Boundaries

Lookout owns web evidence. anymd's `url` source exists so an agent with a single
PDF link does not have to fetch it first.
