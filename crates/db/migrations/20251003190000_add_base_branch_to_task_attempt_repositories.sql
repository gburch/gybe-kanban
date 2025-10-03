ALTER TABLE task_attempt_repositories
    ADD COLUMN base_branch TEXT;

UPDATE task_attempt_repositories AS tar
SET base_branch = ta.target_branch
FROM task_attempts AS ta
WHERE tar.task_attempt_id = ta.id
  AND tar.base_branch IS NULL;
