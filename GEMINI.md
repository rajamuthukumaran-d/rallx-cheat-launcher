# Rallx Cheat Launcher — Gemini CLI Instructions

@AGENTS.md

## Gemini CLI specifics

- Treat [`AGENTS.md`](./AGENTS.md) above as the source of truth for project
  conventions and requirements. If you need to update the shared instructions,
  edit `AGENTS.md`, not this file.
- Use the Stitch MCP server for the design-asset workflow described in
  `AGENTS.md`, per the screen IDs listed in `PRD.md`, if the Stitch MCP server
  is configured in this environment.
- Prefer `cargo clippy --all-targets -- -D warnings` and `cargo fmt` as your
  own verification step before reporting a coding task done — don't rely on
  the user to catch lint/format issues.
