import { useNavigate } from 'react-router-dom';
import { useEffect, useState } from 'react';
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

interface ProjectSwitcherProps {
  className?: string;
}

export function ProjectSwitcher({ className }: ProjectSwitcherProps) {
  const { projectId, project } = useProject();
  const navigate = useNavigate();
  const [projects, setProjects] = useState<Project[]>([]);
  const [isLoading, setIsLoading] = useState(false);

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
    navigate(`/projects/${value}/tasks`);
  };

  const handleProjectInfo = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (projectId) {
      navigate(`/projects/${projectId}`);
    }
  };

  // Don't show the switcher if there are no projects or only on projects list page
  if (!projectId || projects.length === 0) {
    return null;
  }

  const activeLabel = project?.name ?? 'Select project';

  return (
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
  );
}
