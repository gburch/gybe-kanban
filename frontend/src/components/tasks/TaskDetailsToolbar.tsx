import { useCallback, useEffect, useMemo, useReducer, useState } from 'react';
import { Play, GitFork } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useTranslation } from 'react-i18next';
import { projectsApi, attemptsApi } from '@/lib/api';
import { openTaskForm } from '@/lib/openTaskForm';
import type {
  GitBranch,
  TaskAttempt,
  TaskWithAttemptStatus,
} from 'shared/types';
import type { ExecutorProfileId } from 'shared/types';

import { useAttemptExecution, useBranchStatus } from '@/hooks';
import { useTaskStopping } from '@/stores/useTaskDetailsUiStore';
import { useProject } from '@/contexts/project-context';

import CreateAttempt from '@/components/tasks/Toolbar/CreateAttempt.tsx';
import CurrentAttempt from '@/components/tasks/Toolbar/CurrentAttempt.tsx';
import GitOperations from '@/components/tasks/Toolbar/GitOperations.tsx';
import { useUserSystem } from '@/components/config-provider';
import { Card } from '../ui/card';

// UI State Management
type UiAction =
  | { type: 'OPEN_CREATE_PR' }
  | { type: 'CLOSE_CREATE_PR' }
  | { type: 'CREATE_PR_START' }
  | { type: 'CREATE_PR_DONE' }
  | { type: 'ENTER_CREATE_MODE' }
  | { type: 'LEAVE_CREATE_MODE' }
  | { type: 'SET_ERROR'; payload: string | null };

interface UiState {
  showCreatePRDialog: boolean;
  creatingPR: boolean;
  userForcedCreateMode: boolean;
  error: string | null;
}

const initialUi: UiState = {
  showCreatePRDialog: false,
  creatingPR: false,
  userForcedCreateMode: false,
  error: null,
};

function uiReducer(state: UiState, action: UiAction): UiState {
  switch (action.type) {
    case 'OPEN_CREATE_PR':
      return { ...state, showCreatePRDialog: true };
    case 'CLOSE_CREATE_PR':
      return { ...state, showCreatePRDialog: false };
    case 'CREATE_PR_START':
      return { ...state, creatingPR: true };
    case 'CREATE_PR_DONE':
      return { ...state, creatingPR: false };
    case 'ENTER_CREATE_MODE':
      return { ...state, userForcedCreateMode: true };
    case 'LEAVE_CREATE_MODE':
      return { ...state, userForcedCreateMode: false };
    case 'SET_ERROR':
      return { ...state, error: action.payload };
    default:
      return state;
  }
}

function TaskDetailsToolbar({
  task,
  projectId,
  projectHasDevScript,
  forceCreateAttempt,
  onLeaveForceCreateAttempt,
  attempts,
  selectedAttempt,
  setSelectedAttempt,
}: {
  task: TaskWithAttemptStatus;
  projectId: string;
  projectHasDevScript?: boolean;
  forceCreateAttempt?: boolean;
  onLeaveForceCreateAttempt?: () => void;
  attempts: TaskAttempt[];
  selectedAttempt: TaskAttempt | null;
  setSelectedAttempt: (attempt: TaskAttempt | null) => void;
}) {
  const { t } = useTranslation('tasks');
  // Use props instead of context
  const taskAttempts = attempts;
  // const { setLoading } = useTaskLoading(task.id);
  const { isStopping } = useTaskStopping(task.id);
  const { isAttemptRunning } = useAttemptExecution(selectedAttempt?.id);
  const { selectedRepositoryId } = useProject();
  const {
    data: branchStatus,
    refetch: refetchBranchStatus,
  } = useBranchStatus(selectedAttempt?.id);

  // UI state using reducer
  const [ui, dispatch] = useReducer(uiReducer, initialUi);

  // Data state
  const [branches, setBranches] = useState<GitBranch[]>([]);
  const [selectedBranch, setSelectedBranch] = useState<string | null>(null);
  const [selectedProfile, setSelectedProfile] =
    useState<ExecutorProfileId | null>(null);
  const [parentTargetBranch, setParentTargetBranch] = useState<string | null>(null);
  // const { attemptId: urlAttemptId } = useParams<{ attemptId?: string }>();
  const { system, profiles } = useUserSystem();

  // Memoize latest attempt calculation
  const latestAttempt = useMemo(() => {
    if (taskAttempts.length === 0) return null;
    return taskAttempts.reduce((latest, current) =>
      new Date(current.created_at) > new Date(latest.created_at)
        ? current
        : latest
    );
  }, [taskAttempts]);

  // Derived state
  const isInCreateAttemptMode =
    forceCreateAttempt ??
    (ui.userForcedCreateMode || taskAttempts.length === 0);

  // Derive createAttemptBranch for backward compatibility
  const createAttemptBranch = useMemo(() => {
    // Priority order:
    // 1. User explicitly selected a branch
    if (selectedBranch) {
      return selectedBranch;
    }

    // 2. Latest attempt's base branch (existing behavior for resume/rerun)
    if (
      latestAttempt?.target_branch &&
      branches.some((b: GitBranch) => b.name === latestAttempt.target_branch)
    ) {
      return latestAttempt.target_branch;
    }

    // 3. Parent task attempt's base branch (NEW - for inherited tasks)
    if (parentTargetBranch) {
      return parentTargetBranch;
    }

    // 4. Fall back to current branch
    const currentBranch = branches.find((b) => b.is_current);
    return currentBranch?.name || null;
  }, [latestAttempt, branches, selectedBranch, parentTargetBranch]);

  const fetchProjectBranches = useCallback(async () => {
    const result = await projectsApi.getBranches(
      projectId,
      selectedRepositoryId ?? undefined
    );

    setBranches(result);
  }, [projectId, selectedRepositoryId]);

  useEffect(() => {
    fetchProjectBranches();
  }, [fetchProjectBranches]);

  useEffect(() => {
    setSelectedBranch(null);
  }, [selectedRepositoryId]);

  // Set default executor from config
  useEffect(() => {
    if (system.config?.executor_profile) {
      setSelectedProfile(system.config.executor_profile);
    }
  }, [system.config?.executor_profile]);

  // Fetch parent task attempt's base branch
  useEffect(() => {
    if (task.parent_task_attempt) {
      attemptsApi
        .get(task.parent_task_attempt)
        .then((attempt) => setParentTargetBranch(attempt.target_branch))
        .catch(() => setParentTargetBranch(null));
    } else {
      setParentTargetBranch(null);
    }
  }, [task.parent_task_attempt]);

  // Simplified - hooks handle data fetching and navigation
  // const fetchTaskAttempts = useCallback(() => {
  //   // The useSelectedAttempt hook handles all this logic now
  // }, []);

  // Remove fetchTaskAttempts - hooks handle this now

  // Handle entering create attempt mode
  const handleEnterCreateAttemptMode = useCallback(() => {
    dispatch({ type: 'ENTER_CREATE_MODE' });
  }, []);

  // Stub handlers for backward compatibility with CreateAttempt
  const setCreateAttemptBranch = useCallback(
    (branch: string | null | ((prev: string | null) => string | null)) => {
      if (typeof branch === 'function') {
        setSelectedBranch((prev) => branch(prev));
      } else {
        setSelectedBranch(branch);
      }
      // This is now derived state, so no-op
    },
    []
  );

  const setIsInCreateAttemptMode = useCallback(
    (value: boolean | ((prev: boolean) => boolean)) => {
      const boolValue =
        typeof value === 'function' ? value(isInCreateAttemptMode) : value;
      if (boolValue) {
        dispatch({ type: 'ENTER_CREATE_MODE' });
      } else {
        if (onLeaveForceCreateAttempt) onLeaveForceCreateAttempt();
        dispatch({ type: 'LEAVE_CREATE_MODE' });
      }
    },
    [isInCreateAttemptMode, onLeaveForceCreateAttempt]
  );

  const setError = useCallback((value: string | null) => {
    dispatch({ type: 'SET_ERROR', payload: value });
  }, []);

  useEffect(() => {
    dispatch({ type: 'SET_ERROR', payload: null });
  }, [selectedAttempt?.id]);

  // Wrapper functions for UI state dispatch
  return (
    <>
      <div>
        {ui.error && (
          <div className="mb-4 rounded border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
            {ui.error}
          </div>
        )}
        {isInCreateAttemptMode ? (
          <CreateAttempt
            task={task}
            createAttemptBranch={createAttemptBranch}
            selectedBranch={selectedBranch}
            selectedProfile={selectedProfile}
            taskAttempts={taskAttempts}
            branches={branches}
            setCreateAttemptBranch={setCreateAttemptBranch}
            setIsInCreateAttemptMode={setIsInCreateAttemptMode}
            setSelectedProfile={setSelectedProfile}
            availableProfiles={profiles}
            selectedAttempt={selectedAttempt}
          />
        ) : (
          <div className="">
            <Card className="bg-background border-y border-dashed p-3 text-sm">
              Actions
            </Card>
            <div className="p-3">
              {/* Current Attempt Info */}
              <div className="space-y-2">
                {selectedAttempt ? (
                  <CurrentAttempt
                    task={task}
                    projectId={projectId}
                    projectHasDevScript={projectHasDevScript ?? false}
                    selectedAttempt={selectedAttempt}
                    taskAttempts={taskAttempts}
                    branchStatus={branchStatus ?? null}
                    refetchBranchStatus={refetchBranchStatus}
                    handleEnterCreateAttemptMode={handleEnterCreateAttemptMode}
                    setSelectedAttempt={setSelectedAttempt}
                  />
                ) : (
                  <div className="text-center py-8">
                    <div className="text-lg font-medium text-muted-foreground">
                      No attempts yet
                    </div>
                    <div className="text-sm text-muted-foreground mt-1">
                      Start your first attempt to begin working on this task
                    </div>
                  </div>
                )}
              </div>

              {/* Special Actions: show only in sidebar (non-fullscreen) */}
              {!selectedAttempt && !isAttemptRunning && !isStopping && (
                <div className="space-y-2 pt-3 border-t">
                  <Button
                    onClick={handleEnterCreateAttemptMode}
                    size="sm"
                    className="w-full gap-2 bg-black text-white hover:bg-black/90"
                  >
                    <Play className="h-4 w-4" />
                    Start Attempt
                  </Button>
                  <Button
                    onClick={() =>
                      openTaskForm({
                        projectId,
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
              )}
            </div>
          </div>
        )}

        {/* Git operations remain visible even if branch status fails */}
        {selectedAttempt && (
          <div className="mt-3">
            <GitOperations
              selectedAttempt={selectedAttempt}
              task={task}
              projectId={projectId}
              branchStatus={branchStatus ?? null}
              branches={branches}
              isAttemptRunning={isAttemptRunning}
              setError={setError}
              selectedBranch={selectedBranch ?? selectedAttempt.target_branch ?? null}
            />
          </div>
        )}
      </div>
    </>
  );
}

export default TaskDetailsToolbar;
