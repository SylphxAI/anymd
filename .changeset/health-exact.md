---
'@sylphx/anymd': patch
---

HTTP transport: only `GET /mcp/health` skips the API key. Before this fix, any GET path ending in `/health` did, and those requests fell through to the MCP route.
