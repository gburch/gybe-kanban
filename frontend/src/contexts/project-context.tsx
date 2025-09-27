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
import { useQuery } from '@tanstack/react-query';
import { projectsApi } from '@/lib/api';
import type { Project, ProjectRepository } from 'shared/types';

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
}

const ProjectContext = createContext<ProjectContextValue | null>(null);

interface ProjectProviderProps {
  children: ReactNode;
}

export function ProjectProvider({ children }: ProjectProviderProps) {
  const location = useLocation();

  // Extract projectId from current route path
  const projectId = useMemo(() => {
    const match = location.pathname.match(/^\/projects\/([^/]+)/);
    return match ? match[1] : undefined;
  }, [location.pathname]);

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
