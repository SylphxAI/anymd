---
"@sylphx/citra": patch
---

Pin the URL loader's connection to the DNS answer the SSRF guard validated, so a
hostile zone can no longer answer the check with a public address and the connect
with `169.254.169.254` (DNS-rebinding / TOCTOU bypass, GHSA-5r2f-7788-qp8v). Adds
`test/pdf/rebind.test.ts`, which fails without the pin.
