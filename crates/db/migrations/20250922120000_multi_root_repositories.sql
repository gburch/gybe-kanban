PRAGMA foreign_keys = ON;

CREATE TABLE project_repositories (
    id            BLOB PRIMARY KEY,
    project_id    BLOB NOT NULL,
    name          TEXT NOT NULL,
    git_repo_path TEXT NOT NULL,
    root_path     TEXT NOT NULL DEFAULT '',
    is_primary    INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    created_at    TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_project_repositories_project_name
    ON project_repositories(project_id, name);

CREATE UNIQUE INDEX idx_project_repositories_project_primary
    ON project_repositories(project_id)
    WHERE is_primary = 1;

CREATE UNIQUE INDEX idx_project_repositories_path
    ON project_repositories(project_id, git_repo_path, root_path);

CREATE TABLE task_attempt_repositories (
    id                    BLOB PRIMARY KEY,
    task_attempt_id       BLOB NOT NULL,
    project_repository_id BLOB NOT NULL,
    is_primary            INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    container_ref         TEXT,
    branch                TEXT,
    created_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (task_attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE,
    FOREIGN KEY (project_repository_id) REFERENCES project_repositories(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_task_attempt_repositories_unique
    ON task_attempt_repositories(task_attempt_id, project_repository_id);

CREATE UNIQUE INDEX idx_task_attempt_repositories_primary
    ON task_attempt_repositories(task_attempt_id)
    WHERE is_primary = 1;

CREATE INDEX idx_task_attempt_repositories_repo
    ON task_attempt_repositories(project_repository_id);

-- Backfill existing projects into project_repositories
INSERT INTO project_repositories (id, project_id, name, git_repo_path, root_path, is_primary, created_at, updated_at)
SELECT
    randomblob(16),
    id,
    'Primary',
    git_repo_path,
    '',
    1,
    created_at,
    updated_at
FROM projects;

-- Backfill existing task attempts into task_attempt_repositories
INSERT INTO task_attempt_repositories (id, task_attempt_id, project_repository_id, is_primary, container_ref, branch, created_at, updated_at)
SELECT
    randomblob(16),
    ta.id,
    pr.id,
    1,
    ta.container_ref,
    ta.branch,
    ta.created_at,
    ta.updated_at
FROM task_attempts ta
JOIN tasks t ON ta.task_id = t.id
JOIN project_repositories pr ON pr.project_id = t.project_id
WHERE pr.is_primary = 1;
