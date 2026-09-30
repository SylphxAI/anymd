# Remote URL policy — anymd

anymd accepts a `url` source because many agents are handed a PDF link rather
than a path. Remote URLs are in scope, but only as the narrow fetch described
here.

## What a `url` source is for

- One http(s) document body, fetched by the local process and treated exactly like a
  local file after download.
- An explicit convenience for a caller that already has a link.

It is not a browser, a crawler, a general web client, an authentication
delegate, or a way to reach a private network. 

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

## Tests

`crates/anymd-core/src/url_fetch.rs` covers the guarantee, including
`one_dns_resolution_is_pinned_to_the_actual_connection` and the redirect
re-validation tests.

## Boundaries

A `url` source exists so an agent with a single link does not have to fetch it first. It is not a web research tool.
