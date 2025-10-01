import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Project } from 'shared/types';
import { showProjectForm } from '@/lib/modals';
import { projectsApi } from '@/lib/api';
import { AlertCircle, Loader2, Plus } from 'lucide-react';
import ProjectCard from '@/components/projects/ProjectCard.tsx';
import { useKeyCreate, Scope } from '@/keyboard';
import { ProjectActivityFeed } from '@/components/home/ProjectActivityFeed';

export function ProjectList() {
  const { t } = useTranslation('projects');
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [focusedProjectId, setFocusedProjectId] = useState<string | null>(null);

  const fetchProjects = async () => {
    setLoading(true);
    setError('');

    try {
      const result = await projectsApi.getAll();
      setProjects(result);
    } catch (error) {
      console.error('Failed to fetch projects:', error);
      setError(t('errors.fetchFailed'));
    } finally {
      setLoading(false);
    }
  };

  const handleCreateProject = async () => {
    try {
      const result = await showProjectForm();
      if (result === 'saved') {
        fetchProjects();
      }
    } catch (error) {
      // User cancelled - do nothing
    }
  };

  // Semantic keyboard shortcut for creating new project
  useKeyCreate(handleCreateProject, { scope: Scope.PROJECTS });

  const handleEditProject = async (project: Project) => {
    try {
      const result = await showProjectForm({ project });
      if (result === 'saved') {
        fetchProjects();
      }
    } catch (error) {
      // User cancelled - do nothing
    }
  };

  // Set initial focus when projects are loaded
  useEffect(() => {
    if (projects.length > 0 && !focusedProjectId) {
      setFocusedProjectId(projects[0].id);
    }
  }, [projects, focusedProjectId]);

  useEffect(() => {
    fetchProjects();
  }, []);

  return (
    <div className="p-8 pb-16 md:pb-8 h-full overflow-auto">
      <div className="flex justify-between items-center">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">{t('title')}</h1>
          <p className="text-muted-foreground">{t('subtitle')}</p>
        </div>
        <Button onClick={handleCreateProject}>
          <Plus className="mr-2 h-4 w-4" />
          {t('createProject')}
        </Button>
      </div>

      <div className="mt-6 grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(280px,1fr)]">
        <div className="space-y-6">
          {error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}

          {loading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              {t('loading')}
            </div>
          ) : projects.length === 0 ? (
            <Card>
              <CardContent className="py-12 text-center">
                <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-lg bg-muted">
                  <Plus className="h-6 w-6" />
                </div>
                <h3 className="mt-4 text-lg font-semibold">{t('empty.title')}</h3>
                <p className="mt-2 text-sm text-muted-foreground">
                  {t('empty.description')}
                </p>
                <Button className="mt-4" onClick={handleCreateProject}>
                  <Plus className="mr-2 h-4 w-4" />
                  {t('empty.createFirst')}
                </Button>
              </CardContent>
            </Card>
          ) : (
            <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-3">
              {projects.map((project) => (
                <ProjectCard
                  key={project.id}
                  project={project}
                  isFocused={focusedProjectId === project.id}
                  setError={setError}
                  onEdit={handleEditProject}
                  fetchProjects={fetchProjects}
                />
              ))}
            </div>
          )}
        </div>
        <aside className="hidden lg:block">
          <ProjectActivityFeed
            projects={projects.map(({ id, name }) => ({ id, name }))}
            projectId={projects[0]?.id ?? null}
            isProjectsLoading={loading}
            className="sticky top-8"
          />
        </aside>
      </div>

      <div className="mt-6 lg:hidden">
        <ProjectActivityFeed
          projects={projects.map(({ id, name }) => ({ id, name }))}
          projectId={projects[0]?.id ?? null}
          isProjectsLoading={loading}
        />
      </div>
    </div>
  );
}
