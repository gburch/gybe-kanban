# Integration Plan: Align Fork with origin/main (October 2025)

## Epic 1 – Restore Task Attempt Branch Contract & Database Alignment

### Context
Our fork currently allows `CreateTaskAttempt.branch` to be `Option<String>` and `TaskAttempt::create` fabricates a missing branch by cloning `base_branch`. Upstream (`origin/main`) requires callers to generate unique branches via `Deployment::container().git_branch_from_task_attempt`, and the database enforces `task_attempts.branch` NOT NULL (renamed `target_branch`). This divergence causes new attempts to fail with “NOT NULL constraint failed: task_attempts.branch” and breaks sqlx metadata. All remediation must be performed without data loss; we must preserve every existing `task_attempts` row and associated repositories/migrations.

### Deliverables
1. `CreateTaskAttempt` (Rust) restored to `pub branch: String` and mirrored in generated TS types (`shared/types.ts`).
2. `TaskAttempt::create` signature updated to `(pool, data, attempt_id, task_id)` and inserts both the provided branch and target branch. The method should no longer mint its own UUID.
3. All server/local callers updated to supply `attempt_id` + generated branch:
   - `crates/server/src/routes/tasks.rs::create_task_and_start`
   - `crates/server/src/routes/task_attempts.rs::create_task_attempt`
   - Local deployment bootstrap (`crates/local-deployment/src/container.rs`) and any tests/fixtures that construct `CreateTaskAttempt`.
4. Multi-repo helpers/tests (e.g., `crates/db/tests/multi_repo_flows.rs`) updated to pass explicit branch strings.
5. Regenerated sqlx cache (`cargo sqlx prepare` with `DATABASE_URL=sqlite://dev_assets/db.sqlite`) to reflect new insert statement and target_branch rename.
6. Re-run `npm run generate-types` so the frontend consumes the new `branch: string` requirement.

### Steps
- Modify Rust struct & method signatures.
- Update call sites/tests/helpers to pass `branch` (using `git_branch_from_task_attempt`).
- Regenerate shared types and sqlx metadata.
- Run `cargo check --workspace`, `npm run check`, and relevant unit tests.
- Verify task creation via CLI/API to confirm NOT NULL error is gone.

## Epic 2 – Merge Upstream Multi-Repo & Follow-Up Stream Features

### Context
`origin/main` introduced multi-repository support, follow-up draft streaming changes, approval workflow rewrites, and preview-tab UI. Our fork already has partial multi-repo logic but diverges heavily. A direct merge raises conflicts across DB models, server routes, services, and multiple React components.

### Deliverables
1. Adopt the upstream multi-repo implementation as the baseline: fully merge DB schema and models (ProjectRepository, TaskAttemptRepository, TaskAttempt) consistent with upstream migrations (`20250922120000_multi_root_repositories.sql` etc.), ensuring zero data loss by validating migrations against existing sqlite snapshots before applying.
2. Server routes reconciled:
   - `task_attempts.rs` – ensure branch generation, follow-up retry logic, and draft clearing align while preserving our fork-specific behaviour (e.g., REST polling fallback if still desired).
   - `approvals.rs`/`services::approvals` – adopt upstream `CreateApprovalRequest` flow while retaining any fork analytics or logging.
3. Services alignment (`container.rs`, `git.rs`, `drafts.rs`) – resolve method signature changes (e.g., `cleanup_action`, `WorktreeResetOptions`).
4. Frontend parity:
   - Project dialog uses `useProjectMutations`, repository management UI, and new preview/ClickedElements components.
   - Task creation dialog integrates repository selection + template branch selection using upstream structure; ensure we reapply fork-specific behaviours (e.g., quickstart, follow-up hooks).
   - Follow-up components adopt upstream streaming logic but keep our fallback/polling as feature flag if still required.
5. Delete or adapt fork-only files that duplicate upstream functionality; ensure no regressions in feature coverage (task previews, approvals banners, etc.), and confirm all data persisted in sqlite remains intact after each refactor. Blend any fork-specific enhancements back on top of the upstream implementation only where they provide clear value.

### Steps
- Create a dedicated merge branch (`git checkout -b integrate/origin-main`).
- Replay Epic 1 changes if not yet on top of `origin/main`.
- Merge `origin/main` incrementally, resolving conflicts per subsystem and choosing upstream logic by default—only reinstate fork-specific changes when they demonstrably improve behaviour.
  1. **Database Layer** – models & migrations; run `cargo check -p db` after each.
  2. **Server Routes** – `task_attempts.rs`, `tasks.rs`, approvals endpoints.
  3. **Services** – `container.rs`, `approvals.rs`, `drafts.rs`, `events`.
  4. **Frontend** – project dialog, task dialog, follow-up sections, new preview features.
  5. **Shared Types** – regenerate `shared/types.ts` and confirm frontend build.
- After each subsystem resolution, run targeted checks (`cargo test`, `npm run lint`, `npm run check`) and back up the sqlite dev DB so we can roll back if any step threatens data integrity.
- When the merge completes cleanly, smoke-test core flows: project creation, multi-repo selection, task create/start, follow-up send, approval prompt, preview tab.

## Epic 3 – Reconcile Follow-Up Streaming vs REST Polling

### Context
Our fork temporarily downgraded follow-up updates to REST polling to avoid websocket overload. Upstream stabilized streaming (new `useDraftStream` & server events). We need to decide whether to keep polling, adopt upstream streaming, or expose a configuration toggle.

### Deliverables
1. Documented decision (streaming vs polling) with rationale, including data retention considerations (e.g., ensuring no follow-up drafts are lost during transition).
2. If adopting streaming: restore upstream `useJsonPatchWsStream` usage, ensure server endpoints (`tasks/stream/ws`, etc.) behave under our load, and remove temporary polling hacks.
3. If keeping polling: gate our fallback behind feature flag/environment variable and retain upstream streaming code path for future switch.
4. Update frontend tests/hooks to match chosen behaviour.

### Steps
- Review upstream fixes for websocket overload (buffering, throttling) and confirm applicability.
- Benchmark locally: run task follow-up interactions under both strategies.
- Implement chosen approach and update documentation/config.

## Epic 4 – Approval Workflow Stabilization

### Context
Approvals now rely on executor sessions, normalization patches, and new `CreateApprovalRequest`. Our fork’s version still references legacy `EXIT_PLAN_MODE_TOOL_NAME` logic and direct DB updates. We need to align while keeping any fork analytics hooks.

### Deliverables
1. `services::approvals` matches upstream structure (no `EXIT_PLAN_MODE_TOOL_NAME` check unless reintroduced deliberately) while guaranteeing that existing approval state in the database is preserved.
2. Server approval routes produce/consume `CreateApprovalRequest` and update msg stores correctly.
3. Frontend components (`PendingApprovalEntry`, conversation rendering) align with upstream state machine.
4. Regression tests for approval creation, timeout, rejection, and messaging updates.

### Steps
- Diff our fork vs upstream approvals service to identify intentional customizations.
- Reapply necessary custom behaviour after upstream merge (e.g., analytics, logging) in isolated helper functions.
- Update frontend conversation rendering to match upstream icons/status indicators.
- Validate end-to-end by simulating plan approvals.

## Epic 5 – Documentation & Operational Readiness

### Context
Large integration changes require updated docs and onboarding flow.

### Deliverables
1. Update `docs/` to describe multi-repo rollout, follow-up behaviour, approvals, and task preview features, highlighting the “no data loss” requirement and backup procedures for live environments.
2. Provide ops checklist for migrating existing sqlite DBs (run migrations, regenerate sqlx metadata, ensure branch columns populated).
3. Communicate workflow changes to developers (README, AGENTS.md) – include new commands (e.g., `npm run generate-types`, `cargo sqlx prepare`).
4. Ensure CI scripts (GitHub workflows) align with upstream (pre-release/test actions).

### Steps
- Review upstream docs additions (`multi-repo-rollout-checklist` etc.) and merge relevant content into our `docs/` tree.
- Document migration commands in `docs/operations` and add release notes entry.
- Update CI configuration if upstream shifted (e.g., new lint/test matrix).

---

## Recommended Execution Order
1. **Epic 1** (branch contract) – prerequisite for everything else.
2. **Epic 2** (large merge) – may span multiple PRs; consider `git rerere` to ease conflict resolution.
3. **Epic 3** (follow-up streaming) & **Epic 4** (approvals) – may be tackled in parallel once Epic 2 is stable.
4. **Epic 5** – finalize once code-level changes are merged.

Maintain a running log in this file as each epic progresses (add dates, owners, notes). Treat each epic as a milestone with its own PR(s) and QA sign-off.
