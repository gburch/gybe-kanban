import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import DiffCard from '@/components/DiffCard';

const mockUseUserSystem = vi.fn().mockReturnValue({ config: { theme: 'light' } });
const mockUseReview = vi.fn().mockReturnValue({
  comments: [],
  drafts: {},
  setDraft: vi.fn(),
  addComment: vi.fn(),
  deleteComment: vi.fn(),
  updateComment: vi.fn(),
});
const mockUseDiffViewMode = vi.fn().mockReturnValue('unified');
const defaultProjectContext = {
  projectId: 'proj-1',
  repositoriesById: {
    'repo-123': {
      id: 'repo-123',
      project_id: 'proj-1',
      name: 'Docs Repo',
      git_repo_path: '/workspace/docs',
      root_path: 'docs',
      is_primary: false,
      created_at: new Date('2025-09-01T00:00:00Z') as unknown as Date,
      updated_at: new Date('2025-09-12T00:00:00Z') as unknown as Date,
    },
  },
};

const mockUseProject = vi.fn().mockReturnValue(defaultProjectContext);

vi.mock('@/components/config-provider', () => ({ useUserSystem: () => mockUseUserSystem() }));
vi.mock('@/contexts/ReviewProvider', () => ({ useReview: () => mockUseReview() }));
vi.mock('@/stores/useDiffViewStore', () => ({ useDiffViewMode: () => mockUseDiffViewMode() }));
vi.mock('@/contexts/project-context', () => ({ useProject: () => mockUseProject() }));

// Avoid heavy diff generation by mocking module
vi.mock('@git-diff-view/file', () => ({
  generateDiffFile: vi.fn(() => ({
    initRaw: vi.fn(),
    additionLength: 0,
    deletionLength: 0,
  })),
}));

vi.mock('@git-diff-view/react', () => ({ DiffView: () => null, DiffModeEnum: { Unified: 'unified', Split: 'split' }, SplitSide: { old: 'old', new: 'new' } }));

const baseDiff = {
  repositoryId: 'repo-123',
  repositoryName: 'Docs Repo',
  repositoryRoot: 'docs',
  change: 'modified' as const,
  oldPath: 'docs/intro.md',
  newPath: 'docs/intro.md',
  oldContent: '# Intro',
  newContent: '# Intro Updated',
  contentOmitted: true,
  additions: 3,
  deletions: 1,
};

describe('DiffCard', () => {
  beforeEach(() => {
    mockUseProject.mockReturnValue(defaultProjectContext);
  });

  it('renders repository badge when repository metadata is present', () => {
    render(
      <DiffCard
        diff={baseDiff}
        expanded
        onToggle={() => undefined}
        selectedAttempt={null}
      />
    );

    expect(screen.getByText('Docs Repo')).toBeInTheDocument();
  });

  it('prefers project context name over diff metadata when available', () => {
    render(
      <DiffCard
        diff={{ ...baseDiff, repositoryName: 'Outdated Name' }}
        expanded
        onToggle={() => undefined}
        selectedAttempt={null}
      />
    );

    expect(screen.getByText('Docs Repo')).toBeInTheDocument();
    expect(screen.queryByText('Outdated Name')).not.toBeInTheDocument();
  });

  it('falls back to diff repository name when context is missing', () => {
    mockUseProject.mockReturnValue({
      projectId: 'proj-1',
      repositoriesById: {},
    });

    render(
      <DiffCard
        diff={{ ...baseDiff, repositoryId: 'repo-999', repositoryName: 'Remote Repo' }}
        expanded
        onToggle={() => undefined}
        selectedAttempt={null}
      />
    );

    expect(screen.getByText('Remote Repo')).toBeInTheDocument();
  });
});
