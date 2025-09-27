import {
  createContext,
  useContext,
  ReactNode,
  useMemo,
  useState,
  useEffect,
  useCallback,
} from 'react';
import { useLocation } from 'react-router-dom';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { projectsApi } from '@/lib/api';
import type {
  Project,
  ProjectRepository,
  CreateProjectRepository,
  UpdateProjectRepository,
} from 'shared/types';

interface ProjectContextValue {
  projectId: string | undefined;
  project: Project | undefined;
  isLoading: boolean;
  error: Error | null;
  isError: boolean;
  repositories: ProjectRepository[];
  isRepositoriesLoading: boolean;
  repositoriesError: Error | null;
  selectedRepositoryId: string | null;
  setSelectedRepositoryId: (repositoryId: string | null) => void;
  activeRepository: ProjectRepository | null;
  repositoriesById: Record<string, ProjectRepository>;
  createRepository: (
    data: CreateProjectRepository
  ) => Promise<ProjectRepository>;
  updateRepository: (
    repositoryId: string,
    data: UpdateProjectRepository
  ) => Promise<ProjectRepository>;
  deleteRepository: (repositoryId: string) => Promise<void>;
  isRepositoryMutating: boolean;
}

const ProjectContext = createContext<ProjectContextValue | null>(null);

interface ProjectProviderProps {
  children: ReactNode;
}

export function ProjectProvider({ children }: ProjectProviderProps) {
  const location = useLocation();
  const queryClient = useQueryClient();

  // Extract projectId from current route path
  const projectId = useMemo(() => {
    const match = location.pathname.match(/^\/projects\/([^/]+)/);
    return match ? match[1] : undefined;
  }, [location.pathname]);

  const repositoryCacheKey = useMemo(() => {
    if (!projectId) return null;
    return ['project', projectId, 'repositories'] as const;
  }, [projectId]);

  const projectQuery = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => projectsApi.getById(projectId!),
    enabled: !!projectId,
    staleTime: 5 * 60 * 1000, // 5 minutes
  });

  const repositoriesQuery = useQuery({
    queryKey: ['project', projectId, 'repositories'],
    queryFn: () => projectsApi.getRepositories(projectId!),
    enabled: !!projectId,
    staleTime: 5 * 60 * 1000,
  });

  const [selectedRepositoryId, setSelectedRepositoryId] = useState<string | null>(
    null
  );
  const [isRepositoryMutating, setIsRepositoryMutating] = useState(false);

  const applyRepositoryUpsert = useCallback(
    (list: ProjectRepository[], repository: ProjectRepository) => {
      const existingIndex = list.findIndex((repo) => repo.id === repository.id);

      const demoted = repository.is_primary
        ? list.map((repo) =>
            repo.id !== repository.id && repo.is_primary
              ? { ...repo, is_primary: false }
              : repo
          )
        : list;

      const withoutTarget = demoted.filter(
        (repo) => repo.id !== repository.id
      );

      if (repository.is_primary) {
        return [repository, ...withoutTarget];
      }

      if (existingIndex >= 0) {
        const next = withoutTarget.slice();
        next.splice(Math.min(existingIndex, next.length), 0, repository);
        return next;
      }

      return [...withoutTarget, repository];
    },
    []
  );

  const applyRepositoryRemoval = useCallback(
    (list: ProjectRepository[], repositoryId: string) =>
      list.filter((repo) => repo.id !== repositoryId),
    []
  );

  // Reset selection when project changes entirely
  useEffect(() => {
    setSelectedRepositoryId(null);
  }, [projectId]);

  // Sync selection with repository list
  useEffect(() => {
    const repositories = repositoriesQuery.data;
    if (!repositories || repositories.length === 0) {
      setSelectedRepositoryId(null);
      return;
    }

    setSelectedRepositoryId((current) => {
      if (current && repositories.some((repo) => repo.id === current)) {
        return current;
      }

      const primary =
        repositories.find((repo) => repo.is_primary) ?? repositories[0];
      return primary?.id ?? null;
    });
  }, [repositoriesQuery.data]);

  const handleRepositoryChange = useCallback((repositoryId: string | null) => {
    setSelectedRepositoryId(repositoryId);
  }, []);

  const repositoriesById = useMemo(() => {
    if (!repositoriesQuery.data) {
      return {} as Record<string, ProjectRepository>;
    }
    return repositoriesQuery.data.reduce<Record<string, ProjectRepository>>(
      (acc, repo) => {
        acc[repo.id] = repo;
        return acc;
      },
      {}
    );
  }, [repositoriesQuery.data]);

  const activeRepository = useMemo(() => {
    if (!selectedRepositoryId) return null;
    return repositoriesById[selectedRepositoryId] ?? null;
  }, [repositoriesById, selectedRepositoryId]);

  const createRepository = useCallback(
    async (data: CreateProjectRepository) => {
      if (!projectId || !repositoryCacheKey) {
        throw new Error('A project must be selected before managing repositories.');
      }

      setIsRepositoryMutating(true);

      try {
        const repository = await projectsApi.createRepository(projectId, data);

        queryClient.setQueryData<ProjectRepository[]>(
          repositoryCacheKey,
          (prev) => {
            const current = prev ?? [];
            return applyRepositoryUpsert(current, repository);
          }
        );

        setSelectedRepositoryId((current) => {
          if (repository.is_primary) {
            return repository.id;
          }

          return current ?? repository.id;
        });

        return repository;
      } catch (error) {
        throw error;
      } finally {
        setIsRepositoryMutating(false);
      }
    },
    [applyRepositoryUpsert, projectId, queryClient, repositoryCacheKey]
  );

  const updateRepository = useCallback(
    async (repositoryId: string, data: UpdateProjectRepository) => {
      if (!projectId || !repositoryCacheKey) {
        throw new Error('A project must be selected before managing repositories.');
      }

      setIsRepositoryMutating(true);

      const previous = queryClient.getQueryData<ProjectRepository[]>(
        repositoryCacheKey
      );

      if (previous) {
        const current = previous.find((repo) => repo.id === repositoryId);
          if (current) {
          const optimistic: ProjectRepository = {
            ...current,
            ...(data.name !== undefined && data.name !== null
              ? { name: data.name }
              : {}),
            ...(data.git_repo_path !== undefined && data.git_repo_path !== null
              ? { git_repo_path: data.git_repo_path }
              : {}),
            ...(data.root_path !== undefined && data.root_path !== null
              ? { root_path: data.root_path }
              : {}),
            ...(data.is_primary !== undefined && data.is_primary !== null
              ? { is_primary: data.is_primary }
              : {}),
          };

          queryClient.setQueryData<ProjectRepository[] | undefined>(
            repositoryCacheKey,
            (prev) => {
              const source = prev ?? [];
              return applyRepositoryUpsert(source, optimistic);
            }
          );
        }
      }

      try {
        const repository = await projectsApi.updateRepository(
          projectId,
          repositoryId,
          data
        );

        queryClient.setQueryData<ProjectRepository[]>(
          repositoryCacheKey,
          (prev) => {
            const current = prev ?? [];
            return applyRepositoryUpsert(current, repository);
          }
        );

        if (repository.is_primary) {
          setSelectedRepositoryId(repository.id);
        }

        return repository;
      } catch (error) {
        if (previous) {
          queryClient.setQueryData(repositoryCacheKey, previous);
        }
        throw error;
      } finally {
        setIsRepositoryMutating(false);
      }
    },
    [
      applyRepositoryUpsert,
      projectId,
      queryClient,
      repositoryCacheKey,
    ]
  );

  const deleteRepository = useCallback(
    async (repositoryId: string) => {
      if (!projectId || !repositoryCacheKey) {
        throw new Error('A project must be selected before managing repositories.');
      }

      setIsRepositoryMutating(true);

      const previous = queryClient.getQueryData<ProjectRepository[]>(
        repositoryCacheKey
      );

      queryClient.setQueryData<ProjectRepository[]>(
        repositoryCacheKey,
        (prev) => {
          const source = prev ?? [];
          return applyRepositoryRemoval(source, repositoryId);
        }
      );

      try {
        await projectsApi.deleteRepository(projectId, repositoryId);

        queryClient.setQueryData<ProjectRepository[]>(
          repositoryCacheKey,
          (prev) => {
            const source = prev ?? [];
            return applyRepositoryRemoval(source, repositoryId);
          }
        );

        setSelectedRepositoryId((current) => {
          if (current !== repositoryId) {
            return current;
          }

          const remaining = queryClient.getQueryData<ProjectRepository[]>(
            repositoryCacheKey
          );

          if (!remaining || remaining.length === 0) {
            return null;
          }

          const primary =
            remaining.find((repo) => repo.is_primary) ?? remaining[0];
          return primary.id;
        });
      } catch (error) {
        if (previous) {
          queryClient.setQueryData(repositoryCacheKey, previous);
        }
        throw error;
      } finally {
        setIsRepositoryMutating(false);
      }
    },
    [
      applyRepositoryRemoval,
      projectId,
      queryClient,
      repositoryCacheKey,
    ]
  );

  const value = useMemo(
    () => ({
      projectId,
      project: projectQuery.data,
      isLoading: projectQuery.isLoading,
      error: projectQuery.error ?? null,
      isError: projectQuery.isError,
      repositories: repositoriesQuery.data ?? [],
      isRepositoriesLoading: repositoriesQuery.isLoading,
      repositoriesError: repositoriesQuery.error ?? null,
      selectedRepositoryId,
      setSelectedRepositoryId: handleRepositoryChange,
      activeRepository,
      repositoriesById,
      createRepository,
      updateRepository,
      deleteRepository,
      isRepositoryMutating,
    }),
    [
      projectId,
      projectQuery.data,
      projectQuery.isLoading,
      projectQuery.error,
      projectQuery.isError,
      repositoriesQuery.data,
      repositoriesQuery.isLoading,
      repositoriesQuery.error,
      selectedRepositoryId,
      handleRepositoryChange,
      activeRepository,
      repositoriesById,
      createRepository,
      updateRepository,
      deleteRepository,
      isRepositoryMutating,
    ]
  );

  return (
    <ProjectContext.Provider value={value}>{children}</ProjectContext.Provider>
  );
}

export function useProject(): ProjectContextValue {
  const context = useContext(ProjectContext);
  if (!context) {
    throw new Error('useProject must be used within a ProjectProvider');
  }
  return context;
}
