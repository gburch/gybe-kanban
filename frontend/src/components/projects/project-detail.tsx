import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Project, type ProjectRepository } from 'shared/types';
import {
  showProjectForm,
  showRepositoryForm,
  showDeleteRepository,
} from '@/lib/modals';
import { projectsApi } from '@/lib/api';
import {
  AlertCircle,
  ArrowLeft,
  Calendar,
  CheckSquare,
  Clock,
  Folder,
  Edit,
  Loader2,
  Trash2,
  Plus,
  MoreHorizontal,
  Pencil,
  Star,
} from 'lucide-react';
import { useProject } from '@/contexts/project-context';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

interface ProjectDetailProps {
  projectId: string;
  onBack: () => void;
}

export function ProjectDetail({ projectId, onBack }: ProjectDetailProps) {
  const navigate = useNavigate();
  const [project, setProject] = useState<Project | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const {
    repositories,
    isRepositoriesLoading,
    updateRepository,
    isRepositoryMutating,
  } = useProject();
  const [repositoryError, setRepositoryError] = useState<string | null>(null);
  const [pendingRepositoryId, setPendingRepositoryId] = useState<string | null>(
    null
  );

  const fetchProject = useCallback(async () => {
    setLoading(true);
    setError('');

    try {
      const result = await projectsApi.getById(projectId);
      setProject(result);
    } catch (error) {
      console.error('Failed to fetch project:', error);
      // @ts-expect-error it is type ApiError
      setError(error.message || 'Failed to load project');
    }

    setLoading(false);
  }, [projectId]);

  const handleDelete = async () => {
    if (!project) return;
    if (
      !confirm(
        `Are you sure you want to delete "${project.name}"? This action cannot be undone.`
      )
    )
      return;

    try {
      await projectsApi.delete(projectId);
      onBack();
    } catch (error) {
      console.error('Failed to delete project:', error);
      // @ts-expect-error it is type ApiError
      setError(error.message || 'Failed to delete project');
    }
  };

  const handleEditClick = async () => {
    try {
      const result = await showProjectForm({ project });
      if (result === 'saved') {
        fetchProject();
      }
    } catch (error) {
      // User cancelled - do nothing
    }
  };

  const handleAddRepository = async () => {
    setRepositoryError(null);

    try {
      await showRepositoryForm({});
    } catch (err) {
      // Modal rejected or closed; ignore.
    }
  };

  const handleEditRepository = async (repository: ProjectRepository) => {
    setRepositoryError(null);

    try {
      await showRepositoryForm({ repository });
    } catch (err) {
      // Modal rejected or closed; ignore.
    }
  };

  const handleSetPrimary = async (repository: ProjectRepository) => {
    if (repository.is_primary) return;

    setRepositoryError(null);
    setPendingRepositoryId(repository.id);

    try {
      await updateRepository(repository.id, { is_primary: true });
    } catch (err) {
      setRepositoryError(
        err instanceof Error
          ? err.message
          : 'Failed to update primary repository'
      );
    } finally {
      setPendingRepositoryId(null);
    }
  };

  const handleDeleteRepository = async (repository: ProjectRepository) => {
    setRepositoryError(null);

    try {
      await showDeleteRepository({ repository });
    } catch (err) {
      // Modal rejected or closed; ignore.
    }
  };

  useEffect(() => {
    fetchProject();
  }, [fetchProject]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
        Loading project...
      </div>
    );
  }

  if (error || !project) {
    return (
      <div className="space-y-4 py-12 px-4">
        <Button variant="outline" onClick={onBack}>
          <ArrowLeft className="mr-2 h-4 w-4" />
          Back to Projects
        </Button>
        <Card>
          <CardContent className="py-12 text-center">
            <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-lg bg-muted">
              <AlertCircle className="h-6 w-6 text-muted-foreground" />
            </div>
            <h3 className="mt-4 text-lg font-semibold">Project not found</h3>
            <p className="mt-2 text-sm text-muted-foreground">
              {error ||
                "The project you're looking for doesn't exist or has been deleted."}
            </p>
            <Button className="mt-4" onClick={onBack}>
              Back to Projects
            </Button>
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-6 py-12 px-4">
      <div className="flex justify-between items-start">
        <div className="flex items-center space-x-4">
          <Button variant="outline" onClick={onBack}>
            <ArrowLeft className="mr-2 h-4 w-4" />
            Back to Projects
          </Button>
          <div>
            <div className="flex items-center gap-3">
              <h1 className="text-2xl font-bold">{project.name}</h1>
            </div>
            <p className="text-sm text-muted-foreground">
              Project details and settings
            </p>
          </div>
        </div>
        <div className="flex gap-2">
          <Button onClick={() => navigate(`/projects/${projectId}/tasks`)}>
            <CheckSquare className="mr-2 h-4 w-4" />
            View Tasks
          </Button>
          <Button variant="outline" onClick={handleEditClick}>
            <Edit className="mr-2 h-4 w-4" />
            Edit
          </Button>
          <Button
            variant="outline"
            onClick={handleDelete}
            className="text-destructive hover:text-destructive-foreground hover:bg-destructive/10"
          >
            <Trash2 className="mr-2 h-4 w-4" />
            Delete
          </Button>
        </div>
      </div>

      {error && (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <div className="grid gap-6 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center">
              <Calendar className="mr-2 h-5 w-5" />
              Project Information
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium text-muted-foreground">
                Status
              </span>
              <Badge variant="secondary">Active</Badge>
            </div>
            <div className="space-y-2">
              <div className="flex items-center text-sm">
                <Calendar className="mr-2 h-4 w-4 text-muted-foreground" />
                <span className="text-muted-foreground">Created:</span>
                <span className="ml-2">
                  {new Date(project.created_at).toLocaleDateString()}
                </span>
              </div>
              <div className="flex items-center text-sm">
                <Clock className="mr-2 h-4 w-4 text-muted-foreground" />
                <span className="text-muted-foreground">Last Updated:</span>
                <span className="ml-2">
                  {new Date(project.updated_at).toLocaleDateString()}
                </span>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Project Details</CardTitle>
            <CardDescription>
              Technical information about this project
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <div>
              <h4 className="text-sm font-medium text-muted-foreground">
                Project ID
              </h4>
              <code className="mt-1 block text-xs bg-muted p-2 rounded font-mono">
                {project.id}
              </code>
            </div>
            <div>
              <h4 className="text-sm font-medium text-muted-foreground">
                Created At
              </h4>
              <p className="mt-1 text-sm">
                {new Date(project.created_at).toLocaleString()}
              </p>
            </div>
            <div>
              <h4 className="text-sm font-medium text-muted-foreground">
                Last Modified
              </h4>
              <p className="mt-1 text-sm">
                {new Date(project.updated_at).toLocaleString()}
              </p>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center">
              <Folder className="mr-2 h-5 w-5" />
              Repositories
            </CardTitle>
            <CardDescription>
              Connected repositories for this project
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddRepository}
                disabled={isRepositoryMutating}
              >
                <Plus className="mr-2 h-4 w-4" /> Add repository
              </Button>
            </div>

            {repositoryError ? (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>{repositoryError}</AlertDescription>
              </Alert>
            ) : null}

            {isRepositoriesLoading ? (
              <div className="flex items-center text-sm text-muted-foreground">
                <Loader2 className="mr-2 h-4 w-4 animate-spin" /> Loading
                repositories…
              </div>
            ) : repositories.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                No repositories configured yet.
              </p>
            ) : (
              <div className="space-y-3">
                {repositories.map((repo) => (
                  <div
                    key={repo.id}
                    className="rounded border border-border p-3 text-sm"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex flex-col gap-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span
                            className="font-medium truncate"
                            title={repo.name}
                          >
                            {repo.name}
                          </span>
                          {repo.is_primary && (
                            <Badge variant="outline" className="text-[10px]">
                              Primary
                            </Badge>
                          )}
                        </div>
                        <div className="mt-1 text-xs text-muted-foreground break-all">
                          <span className="font-medium">Path:</span>{' '}
                          <span title={repo.git_repo_path}>
                            {repo.git_repo_path}
                          </span>
                        </div>
                        {repo.root_path ? (
                          <div className="mt-1 text-xs text-muted-foreground break-all">
                            <span className="font-medium">Root:</span>{' '}
                            <span title={repo.root_path}>{repo.root_path}</span>
                          </div>
                        ) : null}
                        <div className="mt-1 text-xs text-muted-foreground">
                          Updated{' '}
                          {new Date(repo.updated_at).toLocaleString()}
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          onClick={() => handleEditRepository(repo)}
                          disabled={isRepositoryMutating}
                          aria-label={`Edit ${repo.name}`}
                        >
                          <Pencil className="h-4 w-4" />
                        </Button>
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              disabled={
                                isRepositoryMutating ||
                                pendingRepositoryId === repo.id
                              }
                              aria-label={`Repository actions for ${repo.name}`}
                            >
                              <MoreHorizontal className="h-4 w-4" />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end" className="w-48">
                            {!repo.is_primary ? (
                              <DropdownMenuItem
                                onSelect={(event) => {
                                  event.preventDefault();
                                  handleSetPrimary(repo);
                                }}
                                disabled={
                                  isRepositoryMutating ||
                                  pendingRepositoryId === repo.id
                                }
                              >
                                <Star className="mr-2 h-4 w-4" /> Make primary
                              </DropdownMenuItem>
                            ) : null}
                            {!repo.is_primary && (
                              <DropdownMenuSeparator />
                            )}
                            <DropdownMenuItem
                              onSelect={(event) => {
                                event.preventDefault();
                                handleDeleteRepository(repo);
                              }}
                              className="text-destructive focus:text-destructive"
                            >
                              <Trash2 className="mr-2 h-4 w-4" /> Remove
                            </DropdownMenuItem>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
