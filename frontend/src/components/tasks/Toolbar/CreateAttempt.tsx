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
import { Card } from '@/components/ui/card';
import { Label } from '@/components/ui/label';
import { openTaskForm } from '@/lib/openTaskForm';

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
    repositories,
    selectedRepositoryId,
    setSelectedRepositoryId,
    activeRepository,
  } = useProject();
  const [repositorySelection, setRepositorySelection] = useState<RepositorySelectionValue>({
    selectedIds: [],
    primaryId: null,
  });
  const [selectionError, setSelectionError] = useState<string | null>(null);
  const repositoryLabel = activeRepository?.name ?? 'Primary repository';

  useEffect(() => {
    if (!repositories || repositories.length === 0) {
      setRepositorySelection({ selectedIds: [], primaryId: null });
      return;
    }

    setRepositorySelection((prev) =>
      normalizeRepositorySelection(prev, repositories, selectedRepositoryId)
    );
  }, [repositories, selectedRepositoryId]);

  useEffect(() => {
    if (
      repositorySelection.primaryId === null &&
      repositories &&
      repositories.length > 0
    ) {
      const defaults = buildRepositorySelectionDefaults(
        repositories,
        selectedRepositoryId
      );
      setRepositorySelection(defaults);
      if (defaults.primaryId) {
        setSelectedRepositoryId(defaults.primaryId);
      }
    }
  }, [repositories, repositorySelection.primaryId, selectedRepositoryId, setSelectedRepositoryId]);

  const handleRepositorySelectionChange = useCallback(
    (next: RepositorySelectionValue) => {
      setSelectionError(null);
      setRepositorySelection(next);
      if (next.primaryId) {
        setSelectedRepositoryId(next.primaryId);
      }
    },
    [setSelectedRepositoryId]
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
        repositories: repositorySelection.selectedIds.map((id) => ({
          project_repository_id: id,
          is_primary: repositorySelection.primaryId === id,
        })),
      });
    },
    [createAttempt, repositorySelection, selectedBranch]
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
              onBranchSelect={setCreateAttemptBranch}
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

        {/* Create Subtask Button */}
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
