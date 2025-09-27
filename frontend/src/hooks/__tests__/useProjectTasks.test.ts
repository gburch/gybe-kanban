import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useProjectTasks } from '@/hooks/useProjectTasks';
import type { TaskStatus, TaskWithAttemptStatus } from 'shared/types';

const mockUseJsonPatchWsStream = vi.fn();

vi.mock('@/hooks/useJsonPatchWsStream', () => ({
  useJsonPatchWsStream: (
    endpoint: string,
    enabled: boolean,
    initialData: () => { tasks: Record<string, TaskWithAttemptStatus> }
  ) => mockUseJsonPatchWsStream(endpoint, enabled, initialData),
}));

describe('useProjectTasks', () => {
  const baseTask = (overrides: Partial<TaskWithAttemptStatus>): TaskWithAttemptStatus => ({
    has_in_progress_attempt: false,
    has_merged_attempt: false,
    last_attempt_failed: false,
    executor: 'agent',
    parent_task_id: null,
    child_task_count: BigInt(0),
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
  });

  it('resolves parent metadata when the parent task exists in the payload', () => {
    const parentTask = baseTask({
      id: 'parent-1',
      title: 'Parent task',
      status: 'inprogress' as TaskStatus,
    });

    const childTask = baseTask({
      id: 'child-1',
      title: 'Child task',
      parent_task_id: 'parent-1',
      created_at: new Date('2024-02-01T00:00:00Z').toISOString(),
    });

    mockUseJsonPatchWsStream.mockReturnValue({
      data: { tasks: { [parentTask.id]: parentTask, [childTask.id]: childTask } },
      isConnected: true,
      error: null,
    });

    const { result } = renderHook(() => useProjectTasks('project-1'));

    expect(result.current.parentTasksById[childTask.id]).toEqual({
      id: parentTask.id,
      title: parentTask.title,
      status: parentTask.status,
    });
    expect(result.current.getParentTask(childTask.id)).toEqual({
      id: parentTask.id,
      title: parentTask.title,
      status: parentTask.status,
    });
    expect(result.current.getParentTask(parentTask.id)).toBeNull();
  });

  it('returns null when the parent is not present in the payload', () => {
    const orphanTask = baseTask({ id: 'orphan-1', parent_task_id: 'missing-1' });

    mockUseJsonPatchWsStream.mockReturnValue({
      data: { tasks: { [orphanTask.id]: orphanTask } },
      isConnected: true,
      error: null,
    });

    const { result } = renderHook(() => useProjectTasks('project-1'));

    expect(result.current.parentTasksById[orphanTask.id]).toBeNull();
    expect(result.current.getParentTask(orphanTask.id)).toBeNull();
  });
});
