#!/usr/bin/env node
// @sylphx/citra was renamed to @sylphx/anymd. This alias runs the anymd
// launcher in-process, so argv, stdio, exit code, and signals are exactly
// those of `anymd`: no args starts the MCP stdio server, `citra <file>`
// prints Markdown.
import '@sylphx/anymd';
