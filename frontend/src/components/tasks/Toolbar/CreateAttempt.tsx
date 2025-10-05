import { Dispatch, SetStateAction, useCallback, useEffect, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { X, GitFork } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { GitBranch, Task } from 'shared/types';
import type { ExecutorConfig } from 'shared/types';
import type { ExecutorProfileId } from 'shared/types';
import type { TaskAttempt } from 'shared/types';
import { useAttemptCreation } from '@/hooks/useAttemptCreation';
import { useAttemptExecution } from '@/hooks/useAttemptExecution';
import BranchSelector from '@/components/tasks/BranchSelector.tsx';
import { ExecutorProfileSelector } from '@/components/settings';

import { showModal } from '@/lib/modals';
import { projectsApi } from '@/lib/api';
import { Card } from '@/components/ui/card';
import { Label } from '@/components/ui/label';
import { openTaskForm } from '@/lib/openTaskForm';
import { useProject } from '@/contexts/project-context';
import {
  RepositorySelection,
  RepositorySelectionValue,
  buildRepositorySelectionDefaults,
  normalizeRepositorySelection,
} from '@/components/tasks/RepositorySelection';

type Props = {
  task: Task;
  branches: GitBranch[];
  taskAttempts: TaskAttempt[];
  createAttemptBranch: string | null;
  selectedProfile: ExecutorProfileId | null;
  selectedBranch: string | null;
  setIsInCreateAttemptMode: Dispatch<SetStateAction<boolean>>;
  setCreateAttemptBranch: (branch: string | null) => void;
  setSelectedProfile: Dispatch<SetStateAction<ExecutorProfileId | null>>;
  availableProfiles: Record<string, ExecutorConfig> | null;
  selectedAttempt: TaskAttempt | null;
};

function CreateAttempt({
  task,
  branches,
  taskAttempts,
  createAttemptBranch,
  selectedProfile,
  selectedBranch,
  setIsInCreateAttemptMode,
  setCreateAttemptBranch,
  setSelectedProfile,
  availableProfiles,
  selectedAttempt,
}: Props) {
  const { t } = useTranslation('tasks');
  const { isAttemptRunning } = useAttemptExecution(selectedAttempt?.id);
  const { createAttempt, isCreating } = useAttemptCreation(task.id);
  const {
    projectId,
    repositories,
    selectedRepositoryId,
    setSelectedRepositoryId,
    activeRepository,
  } = useProject();
  const [repositorySelection, setRepositorySelection] = useState<RepositorySelectionValue>({
    selectedIds: [],
    primaryId: null,
    baseBranches: {},
  });
  const [repositoryBranches, setRepositoryBranches] = useState<Record<string, GitBranch[]>>({});
  const [branchLoading, setBranchLoading] = useState<Record<string, boolean>>({});
  const [selectionError, setSelectionError] = useState<string | null>(null);
  const repositoryLabel = activeRepository?.name ?? 'Primary repository';

  const ensureRepoBranches = useCallback(
    async (repoId: string) => {
      if (!projectId) {
        return undefined;
      }

      if (repositoryBranches[repoId]) {
        return repositoryBranches[repoId];
      }

      setBranchLoading((prev) => ({ ...prev, [repoId]: true }));
      try {
        const result = await projectsApi.getBranches(projectId, repoId);
        setRepositoryBranches((prev) => ({ ...prev, [repoId]: result }));
        setRepositorySelection((prev) => {
          if (!prev.selectedIds.includes(repoId)) {
            return prev;
          }
          const current = prev.baseBranches[repoId];
          const normalizedCurrent = current && current.trim().length > 0 ? current : null;
          const normalizedDefault =
            createAttemptBranch && createAttemptBranch.trim().length > 0
              ? createAttemptBranch
              : null;
          const fallback =
            normalizedCurrent ??
            normalizedDefault ??
            result.find((branch) => !branch.is_remote)?.name ??
            result[0]?.name ??
            null;
          if (fallback === normalizedCurrent) {
            return prev;
          }
          return {
            ...prev,
            baseBranches: {
              ...prev.baseBranches,
              [repoId]: fallback,
            },
          };
        });
        return result;
      } catch (error) {
        console.error('Failed to load branches for repository', repoId, error);
        throw error;
      } finally {
        setBranchLoading((prev) => {
          const next = { ...prev };
          delete next[repoId];
          return next;
        });
      }
    },
    [projectId, repositoryBranches, createAttemptBranch]
  );

  useEffect(() => {
    if (!repositories || repositories.length === 0) {
      setRepositorySelection({ selectedIds: [], primaryId: null, baseBranches: {} });
      return;
    }

    setRepositorySelection((prev) =>
      normalizeRepositorySelection(
        prev,
        repositories,
        selectedRepositoryId,
        createAttemptBranch
      )
    );
  }, [repositories, selectedRepositoryId, createAttemptBranch]);

  useEffect(() => {
    if (
      repositorySelection.primaryId === null &&
      repositories &&
      repositories.length > 0
    ) {
      const defaults = buildRepositorySelectionDefaults(
        repositories,
        selectedRepositoryId,
        createAttemptBranch
      );
      setRepositorySelection(defaults);
      if (defaults.primaryId) {
        setSelectedRepositoryId(defaults.primaryId);
        void ensureRepoBranches(defaults.primaryId);
      }
    }
  }, [
    repositories,
    repositorySelection.primaryId,
    selectedRepositoryId,
    setSelectedRepositoryId,
    createAttemptBranch,
    ensureRepoBranches,
  ]);

  useEffect(() => {
    if (repositorySelection.primaryId) {
      void ensureRepoBranches(repositorySelection.primaryId);
    }
  }, [repositorySelection.primaryId, ensureRepoBranches]);

  const handleRepositorySelectionChange = useCallback(
    (next: RepositorySelectionValue) => {
      setSelectionError(null);

      const previousIds = new Set(repositorySelection.selectedIds);
      next.selectedIds.forEach((id) => {
        if (!previousIds.has(id)) {
          void ensureRepoBranches(id);
        }
      });

      if (next.primaryId) {
        setSelectedRepositoryId(next.primaryId);
        if (next.primaryId !== repositorySelection.primaryId) {
          void ensureRepoBranches(next.primaryId);
        }
      }

      setRepositorySelection(next);

      if (next.primaryId) {
        const primaryBase =
          next.baseBranches[next.primaryId] ?? createAttemptBranch ?? null;
        if (primaryBase && primaryBase !== createAttemptBranch) {
          setCreateAttemptBranch(primaryBase);
        }
      }
    },
    [
      ensureRepoBranches,
      repositorySelection.primaryId,
      repositorySelection.selectedIds,
      setSelectedRepositoryId,
      createAttemptBranch,
      setCreateAttemptBranch,
    ]
  );

  const handleGlobalBaseSelect = useCallback(
    (branch: string) => {
      setCreateAttemptBranch(branch);
      setSelectionError(null);
      setRepositorySelection((prev) => {
        const primaryId = prev.primaryId;
        if (!primaryId) {
          return prev;
        }
        return {
          ...prev,
          baseBranches: {
            ...prev.baseBranches,
            [primaryId]: branch,
          },
        };
      });
    },
    [setCreateAttemptBranch]
  );

  // Create attempt logic
  const actuallyCreateAttempt = useCallback(
    async (profile: ExecutorProfileId, baseBranch?: string) => {
      const effectiveBaseBranch = baseBranch || selectedBranch;

      if (!effectiveBaseBranch) {
        throw new Error('Base branch is required to create an attempt');
      }

      if (!repositorySelection.primaryId || repositorySelection.selectedIds.length === 0) {
        setSelectionError('Select at least one repository and a primary repository.');
        return;
      }

      await createAttempt({
        profile,
        baseBranch: effectiveBaseBranch,
        repositories: repositorySelection.selectedIds.map((id) => {
          const override = repositorySelection.baseBranches[id];
          const normalizedBase =
            override && override.trim().length > 0
              ? override
              : createAttemptBranch && createAttemptBranch.trim().length > 0
                ? createAttemptBranch
                : null;

          return {
            project_repository_id: id,
            is_primary: repositorySelection.primaryId === id,
            base_branch: normalizedBase ?? undefined,
          };
        }),
      });
    },
    [createAttempt, repositorySelection, selectedBranch, createAttemptBranch]
  );

  // Handler for Enter key or Start button
  const onCreateNewAttempt = useCallback(
    async (
      profile: ExecutorProfileId,
      baseBranch?: string,
      isKeyTriggered?: boolean
    ) => {
      if (task.status === 'todo' && isKeyTriggered) {
        try {
          const result = await showModal<'confirmed' | 'canceled'>(
            'create-attempt-confirm',
            {
              title: 'Start New Attempt?',
              message:
                'Are you sure you want to start a new attempt for this task? This will create a new session and branch.',
            }
          );

          if (result === 'confirmed') {
            await actuallyCreateAttempt(profile, baseBranch);
            setIsInCreateAttemptMode(false);
          }
        } catch (error) {
          // User cancelled - do nothing
        }
      } else {
        await actuallyCreateAttempt(profile, baseBranch);
        setIsInCreateAttemptMode(false);
      }
    },
    [task.status, actuallyCreateAttempt, setIsInCreateAttemptMode]
  );

  const handleExitCreateAttemptMode = () => {
    setIsInCreateAttemptMode(false);
  };

  const handleCreateAttempt = () => {
    if (!selectedProfile) {
      return;
    }
    onCreateNewAttempt(selectedProfile, createAttemptBranch || undefined);
  };

  return (
    <div className="">
      <Card className="bg-background p-3 text-sm border-y border-dashed">
        Create Attempt
      </Card>
      <div className="space-y-3 p-3">
        <div className="flex items-center justify-between">
          {taskAttempts.length > 0 && (
            <Button
              variant="ghost"
              size="sm"
              onClick={handleExitCreateAttemptMode}
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
        <div className="flex items-center">
          <label className="text-xs font-medium text-muted-foreground">
            Each time you start an attempt, a new session is initiated with your
            selected coding agent, and a git worktree and corresponding task
            branch are created.
          </label>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 items-end">
          {/* Top Row: Executor Profile and Variant (spans 2 columns) */}
          {availableProfiles && (
            <div className="col-span-1 sm:col-span-2">
              <ExecutorProfileSelector
                profiles={availableProfiles}
                selectedProfile={selectedProfile}
                onProfileSelect={setSelectedProfile}
                showLabel={true}
              />
            </div>
          )}

          {/* Bottom Row: Base Branch and Start Button */}
          <div className="space-y-1">
            <div className="flex flex-col">
              <Label className="text-sm font-medium">
                Base branch <span className="text-destructive">*</span>
              </Label>
              <span className="text-xs text-muted-foreground">
                Repository: {repositoryLabel}
              </span>
            </div>
            <BranchSelector
              branches={branches}
              selectedBranch={createAttemptBranch}
              onBranchSelect={handleGlobalBaseSelect}
              placeholder="Select branch"
            />
          </div>

          <div className="space-y-1">
            <Label className="text-sm font-medium opacity-0">Start</Label>
            <Button
              onClick={handleCreateAttempt}
              disabled={
                !selectedProfile ||
                !createAttemptBranch ||
                isAttemptRunning ||
                isCreating
              }
              size="sm"
              className="w-full text-xs gap-2 justify-center bg-black text-white hover:bg-black/90"
              title={
                !createAttemptBranch
                  ? 'Base branch is required'
                  : !selectedProfile
                    ? 'Coding agent is required'
                    : undefined
              }
            >
              {isCreating ? 'Creating...' : 'Start'}
            </Button>
          </div>
        </div>

        {repositories.length > 0 && (
          <div className="space-y-1">
            <Label className="text-sm font-medium">
              Repositories <span className="text-muted-foreground text-xs">(select at least one)</span>
            </Label>
            <RepositorySelection
              repositories={repositories}
              value={repositorySelection}
              onChange={handleRepositorySelectionChange}
              disabled={isCreating || isAttemptRunning}
              branchOptions={repositoryBranches}
              isBranchLoading={branchLoading}
              onEnsureBranches={ensureRepoBranches}
              defaultBaseBranch={createAttemptBranch}
            />
            {selectionError && (
              <p className="text-xs text-destructive">{selectionError}</p>
            )}
          </div>
        )}

        <div className="pt-3 border-t">
          <Button
            onClick={() =>
              openTaskForm({
                projectId: task.project_id,
                parentTaskId: task.id,
              })
            }
            size="sm"
            variant="outline"
            className="w-full gap-2"
            disabled={isCreating || isAttemptRunning}
          >
            <GitFork className="h-4 w-4" />
            {t('actions.createSubtask')}
          </Button>
        </div>
      </div>
    </div>
  );
}

export default CreateAttempt;
