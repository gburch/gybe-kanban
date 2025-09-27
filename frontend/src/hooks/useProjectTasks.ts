import { useCallback, useMemo } from 'react';
import { useJsonPatchWsStream } from './useJsonPatchWsStream';
import type { TaskWithAttemptStatus } from 'shared/types';

type TasksState = {
  tasks: Record<string, TaskWithAttemptStatus>;
};

export type ParentTaskSummary = Pick<
  TaskWithAttemptStatus,
  'id' | 'title' | 'status'
>;

interface UseProjectTasksResult {
  tasks: TaskWithAttemptStatus[];
  tasksById: Record<string, TaskWithAttemptStatus>;
  parentTasksById: Record<string, ParentTaskSummary | null>;
  isLoading: boolean;
  isConnected: boolean;
  error: string | null;
  /**
   * Lookup helper for parent metadata; returns null when the parent is missing
   * from the current websocket payload.
   */
  getParentTask: (taskId: string) => ParentTaskSummary | null;
}

/**
 * Stream tasks for a project via WebSocket (JSON Patch) and expose as array + map.
 * Server sends initial snapshot: replace /tasks with an object keyed by id.
 * Live updates arrive at /tasks/<id> via add/replace/remove operations.
 */
export const useProjectTasks = (projectId: string): UseProjectTasksResult => {
  const endpoint = `/api/tasks/stream/ws?project_id=${encodeURIComponent(projectId)}`;

  const initialData = useCallback((): TasksState => ({ tasks: {} }), []);

  const { data, isConnected, error } = useJsonPatchWsStream(
    endpoint,
    !!projectId,
    initialData
  );

  const tasksById = data?.tasks ?? {};
  const tasks = Object.values(tasksById).sort(
    (a, b) =>
      new Date(b.created_at as unknown as string).getTime() -
      new Date(a.created_at as unknown as string).getTime()
  );

  const parentTasksById = useMemo(() => {
    const lookup: Record<string, ParentTaskSummary | null> = {};

    for (const [taskId, task] of Object.entries(tasksById)) {
      const parentId = task.parent_task_id;

      if (!parentId) {
        lookup[taskId] = null;
        continue;
      }

      const parentTask = tasksById[parentId];

      if (!parentTask) {
        lookup[taskId] = null;
        continue;
      }

      lookup[taskId] = {
        id: parentTask.id,
        title: parentTask.title,
        status: parentTask.status,
      };
    }

    return lookup;
  }, [tasksById]);

  /**
   * Resolve parent metadata for a task id. Returns null when the parent task
   * is absent from the current websocket snapshot.
   */
  const getParentTask = useCallback(
    (taskId: string): ParentTaskSummary | null => parentTasksById[taskId] ?? null,
    [parentTasksById]
  );
  const isLoading = !data && !error; // until first snapshot

  return {
    tasks,
    tasksById,
    parentTasksById,
    isLoading,
    isConnected,
    error,
    getParentTask,
  };
};
