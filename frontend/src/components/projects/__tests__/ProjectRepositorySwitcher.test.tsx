import { describe, it, expect, beforeEach, beforeAll, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ProjectRepositorySwitcher } from '@/components/projects/ProjectRepositorySwitcher';

const setSelectedRepositoryIdMock = vi.fn();

beforeAll(() => {
  Object.defineProperty(Element.prototype, 'hasPointerCapture', {
    value: () => false,
    configurable: true,
  });
  Object.defineProperty(Element.prototype, 'setPointerCapture', {
    value: () => undefined,
    configurable: true,
  });
  Object.defineProperty(Element.prototype, 'releasePointerCapture', {
    value: () => undefined,
    configurable: true,
  });
  Object.defineProperty(Element.prototype, 'scrollIntoView', {
    value: () => undefined,
    configurable: true,
  });
});

const repositories = [
  {
    id: 'repo-1',
    project_id: 'proj-1',
    name: 'Primary Repo',
    git_repo_path: '/workspace/primary',
    root_path: '',
    is_primary: true,
    created_at: new Date('2025-09-01T12:00:00Z') as unknown as Date,
    updated_at: new Date('2025-09-10T12:00:00Z') as unknown as Date,
  },
  {
    id: 'repo-2',
    project_id: 'proj-1',
    name: 'Docs Repo',
    git_repo_path: '/workspace/docs',
    root_path: 'docs',
    is_primary: false,
    created_at: new Date('2025-09-02T12:00:00Z') as unknown as Date,
    updated_at: new Date('2025-09-11T12:00:00Z') as unknown as Date,
  },
];

vi.mock('@/contexts/project-context', () => ({
  useProject: () => ({
    projectId: 'proj-1',
    project: null,
    isLoading: false,
    error: null,
    isError: false,
    repositories,
    isRepositoriesLoading: false,
    repositoriesError: null,
    selectedRepositoryId: repositories[0].id,
    setSelectedRepositoryId: setSelectedRepositoryIdMock,
    activeRepository: repositories[0],
    repositoriesById: repositories.reduce<Record<string, (typeof repositories)[number]>>(
      (acc, repo) => {
        acc[repo.id] = repo;
        return acc;
      },
      {}
    ),
  }),
}));

describe('ProjectRepositorySwitcher', () => {
  beforeEach(() => {
    setSelectedRepositoryIdMock.mockClear();
  });

  it('renders repository name and primary badge', () => {
    render(<ProjectRepositorySwitcher hideIfSingle={false} />);

    expect(screen.getByText('Primary Repo')).toBeInTheDocument();
    expect(screen.getByText('Primary')).toBeInTheDocument();
  });

  it('invokes context setter when a different repository is selected', async () => {
    render(<ProjectRepositorySwitcher hideIfSingle={false} />);

    const trigger = screen.getByRole('combobox', { name: /Active repository/ });
    await userEvent.click(trigger);

    const docsOption = await screen.findByText(/Docs Repo/i);

    await userEvent.click(docsOption);

    expect(setSelectedRepositoryIdMock).toHaveBeenCalledWith('repo-2');
  });
});
