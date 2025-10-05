import { useNavigate } from 'react-router-dom';
import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { Info } from 'lucide-react';
import { useProject } from '@/contexts/project-context';
import { projectsApi } from '@/lib/api';
import type { Project } from 'shared/types';
import { cn } from '@/lib/utils';
import { Scope, useKeyboardShortcut } from '@/keyboard';

interface ProjectSwitcherProps {
  className?: string;
}

const MAX_PROJECT_SHORTCUTS = 9;
const PROJECT_SHORTCUT_FORM_TAGS = ['input', 'textarea', 'select'] as const;

type Platform = 'mac' | 'windows' | 'other';

function detectPlatform(): Platform {
  if (typeof navigator === 'undefined') {
    return 'other';
  }

  const nav = navigator as Navigator & {
    userAgentData?: { platform?: string };
  };

  const platformString = (
    nav.userAgentData?.platform ?? nav.platform ?? ''
  ).toLowerCase();

  if (platformString.includes('mac')) return 'mac';
  if (platformString.includes('win')) return 'windows';
  return 'other';
}

export function ProjectSwitcher({ className }: ProjectSwitcherProps) {
  const { projectId, project } = useProject();
  const navigate = useNavigate();
  const [projects, setProjects] = useState<Project[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const platform = useMemo(() => detectPlatform(), []);

  const navigateToProject = useCallback(
    (value: string) => {
      navigate(`/projects/${value}/tasks`);
    },
    [navigate]
  );

  useEffect(() => {
    const fetchProjects = async () => {
      setIsLoading(true);
      try {
        const result = await projectsApi.getAll();
        setProjects(result);
      } catch (error) {
        console.error('Failed to fetch projects:', error);
      } finally {
        setIsLoading(false);
      }
    };

    fetchProjects();
  }, []);

  const handleChange = (value: string) => {
    navigateToProject(value);
  };

  const handleProjectInfo = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (projectId) {
      navigate(`/projects/${projectId}`);
    }
  };

  const shortcutProjects = useMemo(
    () => projects.slice(0, MAX_PROJECT_SHORTCUTS),
    [projects]
  );

  // Don't show the switcher if there are no projects or only on projects list page
  if (!projectId || projects.length === 0) {
    return null;
  }

  const activeLabel = project?.name ?? 'Select project';

  return (
    <>
      {shortcutProjects.map((proj, index) => (
        <ProjectShortcutBinder
          key={proj.id}
          project={proj}
          index={index}
          currentProjectId={projectId}
          onNavigate={navigateToProject}
          platform={platform}
        />
      ))}
      <div className="flex items-center gap-1">
        <Select
          value={projectId}
          onValueChange={handleChange}
          disabled={isLoading || projects.length === 0}
        >
          <SelectTrigger
            className={cn(
              'h-8 w-[180px] border-none shadow-none focus:ring-0 text-sm font-medium',
              className
            )}
            aria-label="Switch project"
          >
            <SelectValue>
              <span className="truncate" title={activeLabel}>
                {activeLabel}
              </span>
            </SelectValue>
          </SelectTrigger>
          <SelectContent className="w-[220px]">
            <SelectGroup>
              {projects.map((proj) => (
                <SelectItem key={proj.id} value={proj.id}>
                  <span className="text-sm truncate" title={proj.name}>
                    {proj.name}
                  </span>
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={handleProjectInfo}
          aria-label="View project details"
        >
          <Info className="h-4 w-4" />
        </Button>
      </div>
    </>
  );
}

interface ProjectShortcutBinderProps {
  project: Project;
  index: number;
  currentProjectId?: string;
  onNavigate: (projectId: string) => void;
  platform: Platform;
}

function getShortcutKeys(platform: Platform, index: number): string[] {
  const slot = index + 1;

  if (platform === 'mac') {
    return [`ctrl+${slot}`];
  }

  return [`ctrl+alt+${slot}`];
}

function ProjectShortcutBinder({
  project,
  index,
  currentProjectId,
  onNavigate,
  platform,
}: ProjectShortcutBinderProps) {
  const keys = useMemo(() => getShortcutKeys(platform, index), [platform, index]);
  const description = useMemo(
    () => `Switch to project ${index + 1}: ${project.name}`,
    [index, project.name]
  );

  useKeyboardShortcut(
    {
      keys: keys.length === 1 ? keys[0] : keys,
      callback: (event) => {
        event.preventDefault();
        if (project.id !== currentProjectId) {
          onNavigate(project.id);
        }
      },
      description,
      group: 'Projects',
      scope: Scope.PROJECTS,
    },
    {
      preventDefault: true,
      enableOnFormTags: PROJECT_SHORTCUT_FORM_TAGS,
    }
  );

  return null;
}
