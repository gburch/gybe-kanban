import { describe, expect, it, beforeEach, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  MemoryRouter,
  Routes,
  Route,
  useLocation,
  useNavigate,
} from 'react-router-dom';
import type { TaskWithAttemptStatus } from 'shared/types';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';

import { ProjectTasks } from '@/pages/project-tasks';

Object.defineProperty(window.HTMLElement.prototype, 'scrollIntoView', {
  value: vi.fn(),
  writable: true,
});

const mockUseProjectTasks = vi.fn();
const mockUseProject = vi.fn();

vi.mock('@/hooks/useProjectTasks', () => ({
  useProjectTasks: (projectId: string) => mockUseProjectTasks(projectId),
}));

vi.mock('@/contexts/project-context', () => ({
  useProject: () => mockUseProject(),
}));

vi.mock('react-i18next', async () => {
  const actual = await vi.importActual<typeof import('react-i18next')>(
    'react-i18next'
  );
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, options?: Record<string, string>) =>
        options?.defaultValue ?? key,
    }),
  };
});

vi.mock('@tanstack/react-query', async () => {
  const actual = await vi.importActual<
    typeof import('@tanstack/react-query')
  >('@tanstack/react-query');
  return {
    ...actual,
    useQuery: () => ({
      data: [],
      isLoading: false,
      error: null,
      refetch: vi.fn(),
    }),
  };
});

vi.mock('@ebay/nice-modal-react', () => ({
  __esModule: true,
  default: { show: vi.fn() },
}));

vi.mock('@/components/config-provider', () => ({
  useUserSystem: () => ({
    system: {
      config: {
        executor_profile: null,
      },
      environment: null,
      profiles: null,
      capabilities: null,
    },
    config: {
      executor_profile: null,
    },
    environment: null,
    profiles: null,
    capabilities: null,
    updateConfig: vi.fn(),
    saveConfig: vi.fn(),
    updateAndSaveConfig: vi.fn(),
    setEnvironment: vi.fn(),
    setProfiles: vi.fn(),
    setCapabilities: vi.fn(),
    reloadSystem: vi.fn(),
    loading: false,
    githubTokenInvalid: false,
  }),
}));

vi.mock('@/contexts/search-context', () => ({
  useSearch: () => ({
    query: '',
    setQuery: vi.fn(),
    active: true,
    clear: vi.fn(),
    focusInput: vi.fn(),
    registerInputRef: vi.fn(),
  }),
}));

vi.mock('react-hotkeys-hook', () => ({
  useHotkeysContext: () => ({ enableScope: vi.fn(), disableScope: vi.fn() }),
}));

vi.mock('@/hooks/useTaskViewManager', () => ({
  useTaskViewManager: () => {
    const navigate = useNavigate();
    const location = useLocation();
      const isFullscreen = location.pathname.endsWith('/full');
    return {
      isFullscreen,
      toggleFullscreen: vi.fn(),
      buildTaskUrl: (
        projectId: string,
        taskId: string,
        options?: { fullscreen?: boolean }
      ) => {
        const fullscreenSuffix =
          options?.fullscreen ?? isFullscreen ? '/full' : '';
        return `/projects/${projectId}/tasks/${taskId}${fullscreenSuffix}`;
      },
      navigateToTask: (
        projectId: string,
        taskId: string,
        options?: { fullscreen?: boolean; replace?: boolean; state?: unknown }
      ) => {
        const target = `/projects/${projectId}/tasks/${taskId}`;
        navigate(target, {
          replace: options?.replace ?? true,
          state: options?.state,
        });
      },
      navigateToAttempt: vi.fn(),
    };
  },
}));

vi.mock('@/keyboard', () => ({
  Scope: { KANBAN: 'kanban' },
  useKeyCreate: () => {},
  useKeyExit: () => {},
  useKeyFocusSearch: () => {},
  useKeyNavUp: () => {},
  useKeyNavDown: () => {},
  useKeyNavLeft: () => {},
  useKeyNavRight: () => {},
  useKeyOpenDetails: () => {},
  useKeyToggleFullscreen: () => {},
  useKeyDeleteTask: () => {},
}));

vi.mock('@/lib/api', () => ({
  tasksApi: { update: vi.fn() },
  attemptsApi: { getAll: vi.fn(), get: vi.fn() },
  projectsApi: { getBranches: vi.fn().mockResolvedValue([]) },
}));

const LocationDisplay = () => {
  const location = useLocation();
  return <div data-testid="location-path">{location.pathname}</div>;
};

const baseTask = (overrides: Partial<TaskWithAttemptStatus>): TaskWithAttemptStatus => ({
  id: 'task-id',
  project_id: 'proj-1',
  title: 'Task Title',
  description: 'Description',
  status: 'todo',
  parent_task_attempt: null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  has_in_progress_attempt: false,
  has_merged_attempt: false,
  last_attempt_failed: false,
  executor: 'executor',
  ...overrides,
});

describe('ProjectTasks parent pill navigation', () => {
  beforeEach(() => {
    mockUseProjectTasks.mockReset();
    mockUseProject.mockReset();

    const parentTask = baseTask({
      id: 'parent-1',
      title: 'Parent Task',
      status: 'inprogress',
    });

    const childTask = baseTask({
      id: 'child-1',
      title: 'Child Task',
      status: 'todo',
    });

    const tasksById: Record<string, TaskWithAttemptStatus> = {
      [parentTask.id]: parentTask,
      [childTask.id]: childTask,
    };

    mockUseProjectTasks.mockImplementation(() => ({
      tasks: [childTask, parentTask],
      tasksById,
      parentTasksById: {
        [childTask.id]: null,
        [parentTask.id]: null,
      },
      childTaskSummaryById: {},
      isLoading: false,
      isConnected: true,
      error: null,
      getParentTask: vi.fn(() => null),
    }));

    mockUseProject.mockReturnValue({
      project: { id: 'proj-1', dev_script: null },
      isLoading: false,
      error: null,
      repositories: [],
      activeRepository: null,
      repositoriesById: {},
      repositoriesError: null,
      isRepositoriesLoading: false,
      selectedRepositoryId: null,
      setSelectedRepositoryId: vi.fn(),
      createRepository: vi.fn(),
      updateRepository: vi.fn(),
      deleteRepository: vi.fn(),
      isRepositoryMutating: false,
    });
  });

  it('renders without parent navigation pill when parent metadata is unavailable', async () => {
    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={['/projects/proj-1/tasks/child-1']}>
          <LocationDisplay />
          <Routes>
            <Route path="/projects/:projectId/tasks" element={<ProjectTasks />} />
            <Route
              path="/projects/:projectId/tasks/:taskId"
              element={<ProjectTasks />}
            />
            <Route
              path="/projects/:projectId/tasks/:taskId/full"
              element={<ProjectTasks />}
            />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    );

    expect(screen.getByTestId('location-path').textContent).toBe(
      '/projects/proj-1/tasks/child-1'
    );

    expect(
      screen.queryByRole('link', {
        name: 'Open parent task Parent Task',
      })
    ).toBeNull();
  });

  it('renders parent navigation pill when metadata is available', async () => {
    const parentTask = baseTask({
      id: 'parent-1',
      title: 'Parent Task',
      status: 'inreview',
    });

    const childTask = baseTask({
      id: 'child-1',
      title: 'Child Task',
      status: 'todo',
      parent_task_attempt: 'attempt-123',
    });

    const tasksById: Record<string, TaskWithAttemptStatus> = {
      [parentTask.id]: parentTask,
      [childTask.id]: childTask,
    };

    mockUseProjectTasks.mockImplementation(() => ({
      tasks: [childTask, parentTask],
      tasksById,
      parentTasksById: {
        [childTask.id]: {
          id: parentTask.id,
          title: parentTask.title,
          status: parentTask.status,
        },
        [parentTask.id]: null,
      },
      childTaskSummaryById: {},
      isLoading: false,
      isConnected: true,
      error: null,
      getParentTask: vi.fn(() => ({
        id: parentTask.id,
        title: parentTask.title,
        status: parentTask.status,
      })),
    }));

    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={['/projects/proj-1/tasks/child-1']}>
          <LocationDisplay />
          <Routes>
            <Route path="/projects/:projectId/tasks" element={<ProjectTasks />} />
            <Route
              path="/projects/:projectId/tasks/:taskId"
              element={<ProjectTasks />}
            />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    );

    expect(
      await screen.findByRole('link', {
        name: 'Open parent task Parent Task',
      })
    ).toBeInTheDocument();
  });
});
