# Rallx Cheat Launcher — Claude Code Instructions

@AGENTS.md

## Claude Code specifics

- Treat [`AGENTS.md`](./AGENTS.md) above as the source of truth for project
  conventions and requirements. If you need to update the shared instructions,
  edit `AGENTS.md`, not this file.
- Use the Stitch MCP server tools (`mcp__stitch__*`) for the design-asset
  workflow described in `AGENTS.md`, per the screen IDs listed in `PRD.md`.
- Prefer `cargo clippy --all-targets -- -D warnings` and `cargo fmt` as your
  own verification step before reporting a coding task done — don't rely on
  the user to catch lint/format issues.
