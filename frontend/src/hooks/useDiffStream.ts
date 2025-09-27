import { useCallback } from 'react';
import type { PatchType } from 'shared/types';
import { useJsonPatchWsStream } from './useJsonPatchWsStream';

interface DiffState {
  entries: Record<string, PatchType>;
}

interface UseDiffStreamResult {
  data: DiffState | undefined;
  isConnected: boolean;
  error: string | null;
}

export const useDiffStream = (
  attemptId: string | null,
  enabled: boolean,
  repositoryId?: string | null
): UseDiffStreamResult => {
  const query = repositoryId ? `?repo_id=${encodeURIComponent(repositoryId)}` : '';
  const endpoint = attemptId
    ? `/api/task-attempts/${attemptId}/diff/ws${query}`
    : undefined;

  const initialData = useCallback(
    (): DiffState => ({
      entries: {},
    }),
    []
  );

  const { data, isConnected, error } = useJsonPatchWsStream(
    endpoint,
    enabled && !!attemptId,
    initialData
    // No need for injectInitialEntry or deduplicatePatches for diffs
  );

  return { data, isConnected, error };
};
