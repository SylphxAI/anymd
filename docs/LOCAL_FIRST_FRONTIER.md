# Local-first frontier

## Principles

1. **Less dependency** — prefer OS/Rust binaries over heavy npm ML  
2. **Zero config** — works without API keys  
3. **Local first, cloud optional**  
4. **Speed / size / performance**  
5. **Rust first** where engines exist  

See product README + EVIDENCE_CONTRACT for surfaces.

## Non-negotiable

1. Zero API key for default path  
2. Prefer Rust native MCP when present  
3. Few tools; primary path documented in TOOL_SURFACE.md  
4. Cloud / LLM only optional and non-authority  
5. Product SSOT is this repository only (no products monorepo)

## Zero-config CTA

```bash
npx -y @sylphx/anymd
```

Bare invoke starts brand-sole MCP on stdio. Live: the latest `@sylphx/anymd` on npm.
