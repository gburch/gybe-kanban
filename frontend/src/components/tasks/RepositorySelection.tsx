import { useCallback, useId } from 'react';
import { Checkbox } from '@/components/ui/checkbox';
import type { ProjectRepository } from 'shared/types';

export type RepositorySelectionValue = {
  selectedIds: string[];
  primaryId: string | null;
};

type Props = {
  repositories: ProjectRepository[];
  value: RepositorySelectionValue;
  onChange: (value: RepositorySelectionValue) => void;
  disabled?: boolean;
};

export function buildRepositorySelectionDefaults(
  repositories: ProjectRepository[],
  preferredPrimary?: string | null
): RepositorySelectionValue {
  if (!repositories || repositories.length === 0) {
    return { selectedIds: [], primaryId: null };
  }

  const repoById = new Map(repositories.map((repo) => [repo.id, repo]));

  let primaryId: string | null = null;
  if (preferredPrimary && repoById.has(preferredPrimary)) {
    primaryId = preferredPrimary;
  }

  if (!primaryId) {
    const primaryRepo = repositories.find((repo) => repo.is_primary);
    primaryId = primaryRepo?.id ?? repositories[0]?.id ?? null;
  }

  const selectedIds = primaryId ? [primaryId] : [];

  return {
    selectedIds,
    primaryId,
  };
}

export function normalizeRepositorySelection(
  previous: RepositorySelectionValue,
  repositories: ProjectRepository[],
  preferredPrimary?: string | null
): RepositorySelectionValue {
  if (!repositories || repositories.length === 0) {
    return { selectedIds: [], primaryId: null };
  }

  const availableIds = new Set(repositories.map((repo) => repo.id));
  const filteredSelected = previous.selectedIds.filter((id) => availableIds.has(id));
  let primaryId = previous.primaryId && availableIds.has(previous.primaryId)
    ? previous.primaryId
    : null;

  if (filteredSelected.length === 0) {
    return buildRepositorySelectionDefaults(repositories, preferredPrimary);
  }

  if (!primaryId) {
    const preferred = preferredPrimary && availableIds.has(preferredPrimary)
      ? preferredPrimary
      : null;
    if (preferred && filteredSelected.includes(preferred)) {
      primaryId = preferred;
    } else {
      const primaryRepo = repositories.find(
        (repo) => repo.is_primary && filteredSelected.includes(repo.id)
      );
      primaryId = primaryRepo?.id ?? filteredSelected[0];
    }
  }

  return {
    selectedIds: Array.from(new Set(filteredSelected)),
    primaryId,
  };
}

export function RepositorySelection({
  repositories,
  value,
  onChange,
  disabled,
}: Props) {
  const groupName = useId();
  const handleToggle = useCallback(
    (repoId: string, checked: boolean) => {
      const selected = new Set(value.selectedIds);
      if (checked) {
        selected.add(repoId);
      } else {
        selected.delete(repoId);
      }

      if (selected.size === 0) {
        onChange({ selectedIds: [], primaryId: null });
        return;
      }

      let primaryId = value.primaryId;
      if (!primaryId || !selected.has(primaryId)) {
        primaryId = selected.values().next().value ?? null;
      }

      onChange({
        selectedIds: Array.from(selected),
        primaryId,
      });
    },
    [onChange, value]
  );

  const handlePrimaryChange = useCallback(
    (repoId: string) => {
      const selected = new Set(value.selectedIds);
      selected.add(repoId);
      onChange({
        selectedIds: Array.from(selected),
        primaryId: repoId,
      });
    },
    [onChange, value]
  );

  if (!repositories || repositories.length === 0) {
    return null;
  }

  return (
    <div className="space-y-2">
      {repositories.map((repo) => {
        const isSelected = value.selectedIds.includes(repo.id);
        const isPrimary = value.primaryId === repo.id;

        return (
          <div
            key={repo.id}
            className="flex items-center gap-3 rounded-md border border-border/70 px-3 py-2"
          >
            <Checkbox
              checked={isSelected}
              onCheckedChange={(checked) =>
                handleToggle(repo.id, checked === true)
              }
              disabled={disabled}
            />
            <div className="flex-1 space-y-1">
              <div className="text-sm font-medium text-foreground">{repo.name}</div>
              <div className="text-xs text-muted-foreground">
                Repo: <span className="break-all">{repo.git_repo_path}</span>
              </div>
              <div className="text-xs text-muted-foreground">
                Root: {repo.root_path || '/'}
              </div>
            </div>
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <input
                type="radio"
                name={groupName}
                value={repo.id}
                checked={isPrimary}
                onChange={() => handlePrimaryChange(repo.id)}
                disabled={disabled || (!isSelected && !isPrimary)}
                className="h-3.5 w-3.5"
              />
              Primary
            </label>
          </div>
        );
      })}
    </div>
  );
}
