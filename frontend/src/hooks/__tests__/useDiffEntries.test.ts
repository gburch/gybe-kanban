import { describe, it, expect, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useDiffEntries } from '@/hooks/useDiffEntries';

const mockUseDiffStream = vi.fn();

vi.mock('@/hooks/useDiffStream', () => ({
  useDiffStream: (
    attemptId: string | null,
    enabled: boolean,
    repositoryId?: string | null
  ) => mockUseDiffStream(attemptId, enabled, repositoryId),
}));

describe('useDiffEntries', () => {
  const sampleDiffEntry = (repositoryId: string | null, path: string) => ({
    type: 'DIFF' as const,
    content: {
      repositoryId,
      repositoryName: repositoryId ? `${repositoryId}-name` : null,
      repositoryRoot: '',
      change: 'modified' as const,
      oldPath: path,
      newPath: path,
      oldContent: 'old',
      newContent: 'new',
      contentOmitted: true,
      additions: 1,
      deletions: 0,
    },
  });

  mockUseDiffStream.mockImplementation(() => ({
    data: {
      entries: {
        a: sampleDiffEntry('repo-1', 'src/a.ts'),
        b: sampleDiffEntry('repo-2', 'src/b.ts'),
      },
    },
    isConnected: true,
    error: null,
  }));

  it('returns all diffs when no repository filter is provided', () => {
    const { result } = renderHook(() => useDiffEntries('attempt-1', true));
    expect(result.current.diffs).toHaveLength(2);
  });

  it('filters diffs by repository identifier when provided', () => {
    const { result } = renderHook(() =>
      useDiffEntries('attempt-1', true, 'repo-2')
    );
    expect(result.current.diffs).toHaveLength(1);
    expect(result.current.diffs[0].repositoryId).toBe('repo-2');
  });
});
