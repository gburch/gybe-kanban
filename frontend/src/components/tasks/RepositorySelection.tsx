import { useCallback, useEffect, useId } from 'react';
import BranchSelector from '@/components/tasks/BranchSelector';
import { Checkbox } from '@/components/ui/checkbox';
import type { GitBranch, ProjectRepository } from 'shared/types';

export type RepositorySelectionValue = {
  selectedIds: string[];
  primaryId: string | null;
  baseBranches: Record<string, string | null>;
};

type Props = {
  repositories: ProjectRepository[];
  value: RepositorySelectionValue;
  onChange: (value: RepositorySelectionValue) => void;
  disabled?: boolean;
  branchOptions?: Record<string, GitBranch[]>;
  isBranchLoading?: Record<string, boolean>;
  onEnsureBranches?: (repoId: string) => void;
  defaultBaseBranch?: string | null;
};

function sanitizeBase(base: string | null | undefined): string | null {
  if (!base) return null;
  const trimmed = base.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function buildRepositorySelectionDefaults(
  repositories: ProjectRepository[],
  preferredPrimary?: string | null,
  defaultBaseBranch?: string | null
): RepositorySelectionValue {
  if (!repositories || repositories.length === 0) {
    return { selectedIds: [], primaryId: null, baseBranches: {} };
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
  const baseBranches: Record<string, string | null> = {};
  if (primaryId) {
    baseBranches[primaryId] = sanitizeBase(defaultBaseBranch);
  }

  return {
    selectedIds,
    primaryId,
    baseBranches,
  };
}

export function normalizeRepositorySelection(
  previous: RepositorySelectionValue,
  repositories: ProjectRepository[],
  preferredPrimary?: string | null,
  defaultBaseBranch?: string | null
): RepositorySelectionValue {
  if (!repositories || repositories.length === 0) {
    return { selectedIds: [], primaryId: null, baseBranches: {} };
  }

  const availableIds = new Set(repositories.map((repo) => repo.id));
  const filteredSelected = previous.selectedIds.filter((id) => availableIds.has(id));
  let primaryId =
    previous.primaryId && availableIds.has(previous.primaryId)
      ? previous.primaryId
      : null;

  if (filteredSelected.length === 0) {
    return buildRepositorySelectionDefaults(
      repositories,
      preferredPrimary,
      defaultBaseBranch
    );
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

  const uniqueSelected = Array.from(new Set(filteredSelected));
  const baseBranches: Record<string, string | null> = {};
  for (const id of uniqueSelected) {
    baseBranches[id] = sanitizeBase(previous.baseBranches[id]) ?? sanitizeBase(defaultBaseBranch);
  }
  if (primaryId) {
    baseBranches[primaryId] =
      sanitizeBase(previous.baseBranches[primaryId]) ?? sanitizeBase(defaultBaseBranch);
  }

  return {
    selectedIds: uniqueSelected,
    primaryId,
    baseBranches,
  };
}

export function RepositorySelection({
  repositories,
  value,
  onChange,
  disabled,
  branchOptions,
  isBranchLoading,
  onEnsureBranches,
  defaultBaseBranch,
}: Props) {
  const groupName = useId();

  const updateSelection = useCallback(
    (
      selectedIds: Set<string>,
      primaryId: string | null,
      baseBranches: Record<string, string | null>
    ) => {
      onChange({
        selectedIds: Array.from(selectedIds),
        primaryId,
        baseBranches,
      });
    },
    [onChange]
  );

  const handleToggle = useCallback(
    (repoId: string, checked: boolean) => {
      const selected = new Set(value.selectedIds);
      const baseBranches = { ...value.baseBranches };

      if (checked) {
        selected.add(repoId);
        if (!Object.hasOwn(baseBranches, repoId)) {
          baseBranches[repoId] = sanitizeBase(defaultBaseBranch);
        }
      } else {
        selected.delete(repoId);
        delete baseBranches[repoId];
      }

      if (selected.size === 0) {
        updateSelection(new Set(), null, {});
        return;
      }

      let primaryId = value.primaryId;
      if (!primaryId || !selected.has(primaryId)) {
        const next = selected.values().next().value as string | undefined;
        primaryId = next ?? null;
      }

      if (primaryId && !Object.hasOwn(baseBranches, primaryId)) {
        baseBranches[primaryId] = sanitizeBase(defaultBaseBranch);
      }

      updateSelection(selected, primaryId, baseBranches);
    },
    [defaultBaseBranch, updateSelection, value.baseBranches, value.primaryId, value.selectedIds]
  );

  const handlePrimaryChange = useCallback(
    (repoId: string) => {
      const selected = new Set(value.selectedIds);
      selected.add(repoId);
      const baseBranches = { ...value.baseBranches };
      if (!Object.hasOwn(baseBranches, repoId)) {
        baseBranches[repoId] = sanitizeBase(defaultBaseBranch);
      }
      updateSelection(selected, repoId, baseBranches);
    },
    [defaultBaseBranch, updateSelection, value.baseBranches, value.selectedIds]
  );

  const handleBaseBranchChange = useCallback(
    (repoId: string, branch: string | null) => {
      const sanitized = sanitizeBase(branch);
      const baseBranches = { ...value.baseBranches };
      if (sanitized) {
        baseBranches[repoId] = sanitized;
      } else {
        delete baseBranches[repoId];
      }
      updateSelection(new Set(value.selectedIds), value.primaryId, baseBranches);
    },
    [updateSelection, value.baseBranches, value.primaryId, value.selectedIds]
  );

  useEffect(() => {
    if (!onEnsureBranches) {
      return;
    }
    value.selectedIds.forEach((id) => {
      onEnsureBranches(id);
    });
  }, [value.selectedIds, onEnsureBranches]);

  if (!repositories || repositories.length === 0) {
    return null;
  }

  return (
    <div className="space-y-2">
      {repositories.map((repo) => {
        const isSelected = value.selectedIds.includes(repo.id);
        const isPrimary = value.primaryId === repo.id;
        const branches = branchOptions?.[repo.id] ?? [];
        const baseBranch = value.baseBranches[repo.id] ?? null;
        const loadingBranches = isBranchLoading?.[repo.id] ?? false;

        return (
          <div
            key={repo.id}
            className="flex flex-col gap-2 rounded-md border border-border/70 px-3 py-2"
          >
            <div className="flex items-center gap-3">
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

            {isSelected && (
              <div className="pl-7">
                <div className="mb-1 text-xs text-muted-foreground">Base branch</div>
                {loadingBranches && branches.length === 0 ? (
                  <div className="text-xs text-muted-foreground">Loading branches…</div>
                ) : branches.length === 0 ? (
                  <div className="text-xs text-muted-foreground">
                    {onEnsureBranches
                      ? 'No branches found yet; refresh the selection to load branches.'
                      : 'No branches available for this repository.'}
                  </div>
                ) : (
                  <BranchSelector
                    branches={branches}
                    selectedBranch={baseBranch}
                    onBranchSelect={(branch) =>
                      handleBaseBranchChange(repo.id, branch)
                    }
                    placeholder="Select base branch"
                  />
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
