# Repository Guidelines

## Project Structure & Module Organization
- `crates/`: Rust workspace crates — `server` (API + bins), `db` (SQLx models/migrations), `executors`, `services`, `utils`, `deployment`, `local-deployment`.
- `frontend/`: React + TypeScript app (Vite, Tailwind). Source in `frontend/src`.
- `frontend/src/components/dialogs`: Dialog components for the frontend.
- `shared/`: Generated TypeScript types (`shared/types.ts`). Do not edit directly.
- `assets/`, `dev_assets_seed/`, `dev_assets/`: Packaged and local dev assets.
- `npx-cli/`: Files published to the npm CLI package.
- `scripts/`: Dev helpers (ports, DB preparation).

## Multi-Repository Attempts
- Some tasks include additional Git repositories besides this primary worktree. Check `VIBE_REPOSITORY_COUNT` and `VIBE_REPOSITORIES` (comma-separated prefixes) to know how many are available.
- For each prefix, environment variables like `VIBE_REPO_<PREFIX>_PATH`, `VIBE_REPO_<PREFIX>_ROOT`, `VIBE_REPO_<PREFIX>_NAME`, and `VIBE_REPO_<PREFIX>_IS_PRIMARY` describe the worktree path, scoped root, human-readable name, and whether it is the primary repo. `VIBE_PRIMARY_REPO_PREFIX`/`VIBE_PRIMARY_REPO_PATH` point to the default location most commands should run from.
- Always set the `workdir` for shell commands to the correct repository path. Use `cd` only when absolutely necessary, and include the repo name in file references if the change is outside the primary repository.
- Secondary repositories are isolated worktrees; be explicit about which repo a change belongs to in your responses (for example, `core-api/src/lib.rs`). Automated merge/push actions target the primary repo—manually outline follow-up steps for secondary repos if required.
- If a repository uses a root override (`VIBE_REPO_<PREFIX>_ROOT` is non-empty), scope your edits to that subdirectory when summarizing or running commands.

## Managing Shared Types Between Rust and TypeScript

ts-rs allows you to derive TypeScript types from Rust structs/enums. By annotating your Rust types with #[derive(TS)] and related macros, ts-rs will generate .ts declaration files for those types.
When making changes to the types, you can regenerate them using `npm run generate-types`
Do not manually edit shared/types.ts, instead edit crates/server/src/bin/generate_types.rs

## Build, Test, and Development Commands
- Install: `pnpm i`
- Run dev (frontend + backend with ports auto-assigned): `pnpm run dev`
- Backend (watch): `npm run backend:dev:watch`
- Frontend (dev): `npm run frontend:dev`
- Type checks: `npm run check` (frontend) and `npm run backend:check` (Rust cargo check)
- Rust tests: `cargo test --workspace`
- Generate TS types from Rust: `npm run generate-types` (or `generate-types:check` in CI)
- Prepare SQLx (offline): `npm run prepare-db`
- Local NPX build: `npm run build:npx` then `npm pack` in `npx-cli/`

## Coding Style & Naming Conventions
- Rust: `rustfmt` enforced (`rustfmt.toml`); group imports by crate; snake_case modules, PascalCase types.
- TypeScript/React: ESLint + Prettier (2 spaces, single quotes, 80 cols). PascalCase components, camelCase vars/functions, kebab-case file names where practical.
- Keep functions small, add `Debug`/`Serialize`/`Deserialize` where useful.

## Testing Guidelines
- Rust: prefer unit tests alongside code (`#[cfg(test)]`), run `cargo test --workspace`. Add tests for new logic and edge cases.
- Frontend: ensure `npm run check` and `npm run lint` pass. If adding runtime logic, include lightweight tests (e.g., Vitest) in the same directory.

## Security & Config Tips
- Use `.env` for local overrides; never commit secrets. Key envs: `FRONTEND_PORT`, `BACKEND_PORT`, `HOST`, optional `GITHUB_CLIENT_ID` for custom OAuth.
- Dev ports and assets are managed by `scripts/setup-dev-environment.js`.
