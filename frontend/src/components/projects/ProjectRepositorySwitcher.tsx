import { Fragment } from 'react';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Badge } from '@/components/ui/badge';
import { useProject } from '@/contexts/project-context';
import { cn } from '@/lib/utils';

interface ProjectRepositorySwitcherProps {
  className?: string;
  hideIfSingle?: boolean;
  size?: 'default' | 'sm';
  label?: string;
}

export function ProjectRepositorySwitcher({
  className,
  hideIfSingle = true,
  size = 'default',
  label,
}: ProjectRepositorySwitcherProps) {
  const {
    projectId,
    repositories,
    isRepositoriesLoading,
    selectedRepositoryId,
    setSelectedRepositoryId,
    activeRepository,
  } = useProject();

  if (!projectId) return null;

  const hasMultipleRepositories = repositories.length > 1;

  if (hideIfSingle && !hasMultipleRepositories && !isRepositoriesLoading) {
    return null;
  }

  const triggerClasses = cn(
    'w-[220px] justify-between',
    size === 'sm' ? 'h-8 text-xs' : 'h-9 text-sm',
    className
  );

  const handleChange = (value: string) => {
    setSelectedRepositoryId(value || null);
  };

  const activeLabel = activeRepository?.name ?? 'Select repository';

  return (
    <div className="flex flex-col gap-1">
      {label ? (
        <span className="text-xs font-medium text-muted-foreground">
          {label}
        </span>
      ) : null}
      <Select
        value={selectedRepositoryId ?? ''}
        onValueChange={handleChange}
        disabled={isRepositoriesLoading || repositories.length === 0}
      >
        <SelectTrigger
          className={triggerClasses}
          aria-label="Active repository"
        >
          <SelectValue>
            <span className="flex items-center gap-2">
              <span className="truncate" title={activeLabel}>
                {activeLabel}
              </span>
              {activeRepository?.is_primary && (
                <Badge variant="outline" className="text-[10px]">
                  Primary
                </Badge>
              )}
            </span>
          </SelectValue>
        </SelectTrigger>
        <SelectContent className="w-[260px]">
          <SelectGroup>
            {repositories.map((repo) => (
              <SelectItem key={repo.id} value={repo.id}>
                <div className="flex flex-col gap-1 text-left">
                  <span className="flex items-center gap-2 text-sm">
                    <span className="truncate" title={repo.name}>
                      {repo.name}
                    </span>
                    {repo.is_primary && (
                      <Badge variant="outline" className="text-[10px]">
                        Primary
                      </Badge>
                    )}
                  </span>
                  <span className="text-[11px] text-muted-foreground truncate" title={repo.git_repo_path}>
                    {repo.git_repo_path}
                  </span>
                  {repo.root_path ? (
                    <span className="text-[11px] text-muted-foreground truncate" title={repo.root_path}>
                      root: {repo.root_path}
                    </span>
                  ) : (
                    <Fragment />
                  )}
                </div>
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  );
}

export default ProjectRepositorySwitcher;
