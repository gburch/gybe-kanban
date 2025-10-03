import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { useProjectTasks } from '@/hooks/useProjectTasks';
import type { TaskStatus, TaskWithAttemptStatus, TaskAttempt } from 'shared/types';

const mockUseJsonPatchWsStream = vi.fn();
const mockAttemptsApiGet = vi.fn();

vi.mock('@/hooks/useJsonPatchWsStream', () => ({
  useJsonPatchWsStream: (
    endpoint: string,
    enabled: boolean,
    initialData: () => { tasks: Record<string, TaskWithAttemptStatus> }
  ) => mockUseJsonPatchWsStream(endpoint, enabled, initialData),
}));

vi.mock('@/lib/api', () => ({
  attemptsApi: {
    get: (attemptId: string) => mockAttemptsApiGet(attemptId),
  },
}));

describe('useProjectTasks', () => {
  const baseTask = (overrides: Partial<TaskWithAttemptStatus>): TaskWithAttemptStatus => ({
    has_in_progress_attempt: false,
    has_running_dev_server: false,
    has_merged_attempt: false,
    last_attempt_failed: false,
    executor: 'agent',
    id: 'task-id',
    project_id: 'project-1',
    title: 'Task title',
    description: null,
    status: 'todo' as TaskStatus,
    parent_task_attempt: null,
    created_at: new Date('2024-01-01T00:00:00Z').toISOString(),
    updated_at: new Date('2024-01-01T00:00:00Z').toISOString(),
    ...overrides,
  });

  beforeEach(() => {
    mockUseJsonPatchWsStream.mockReset();
    mockAttemptsApiGet.mockReset();
  });

  it('resolves parent metadata via parent task attempt lookups', async () => {
    const parentTask = baseTask({
      id: 'parent-1',
      title: 'Parent task',
      status: 'inreview' as TaskStatus,
    });

    const childTask = baseTask({
      id: 'child-1',
      title: 'Child task',
      parent_task_attempt: 'attempt-123',
      created_at: new Date('2024-02-01T00:00:00Z').toISOString(),
    });

    const attemptStub: TaskAttempt = {
      id: 'attempt-123',
      task_id: parentTask.id,
      container_ref: null,
      branch: 'feature/child',
      target_branch: 'main',
      executor: 'executor',
      worktree_deleted: false,
      setup_completed_at: null,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };

    mockAttemptsApiGet.mockResolvedValue(attemptStub);

    mockUseJsonPatchWsStream.mockReturnValue({
      data: { tasks: { [parentTask.id]: parentTask, [childTask.id]: childTask } },
      isConnected: true,
      error: null,
    });

    const { result } = renderHook(() => useProjectTasks('project-1'));

    await waitFor(() => {
      expect(result.current.parentTasksById[childTask.id]).toEqual({
        id: parentTask.id,
        title: parentTask.title,
        status: parentTask.status,
      });
    });

    expect(result.current.getParentTask(childTask.id)).toEqual({
      id: parentTask.id,
      title: parentTask.title,
      status: parentTask.status,
    });
    expect(result.current.getParentTask(parentTask.id)).toBeNull();
  });

  it('computes child task summaries for parent tasks', async () => {
    const parentTask = baseTask({ id: 'parent-1', title: 'Parent task' });
    const childTodo = baseTask({
      id: 'child-todo',
      title: 'Child todo',
      parent_task_attempt: 'attempt-todo',
    });
    const childDone = baseTask({
      id: 'child-done',
      title: 'Child done',
      status: 'done' as TaskStatus,
      parent_task_attempt: 'attempt-done',
    });

    const attemptStubs: Record<string, TaskAttempt> = {
      'attempt-todo': {
        id: 'attempt-todo',
        task_id: parentTask.id,
        container_ref: null,
        branch: 'feature/a',
        target_branch: 'main',
        executor: 'executor',
        worktree_deleted: false,
        setup_completed_at: null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      },
      'attempt-done': {
        id: 'attempt-done',
        task_id: parentTask.id,
        container_ref: null,
        branch: 'feature/b',
        target_branch: 'main',
        executor: 'executor',
        worktree_deleted: false,
        setup_completed_at: null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      },
    };

    mockAttemptsApiGet.mockImplementation((attemptId: string) =>
      Promise.resolve(attemptStubs[attemptId])
    );

    mockUseJsonPatchWsStream.mockReturnValue({
      data: {
        tasks: {
          [parentTask.id]: parentTask,
          [childTodo.id]: childTodo,
          [childDone.id]: childDone,
        },
      },
      isConnected: true,
      error: null,
    });

    const { result } = renderHook(() => useProjectTasks('project-1'));

    await waitFor(() => {
      expect(result.current.childTaskSummaryById[parentTask.id]).toEqual({
        complete: 1,
        inProgress: 0,
        notStarted: 1,
        total: 2,
      });
    });
  });
});
