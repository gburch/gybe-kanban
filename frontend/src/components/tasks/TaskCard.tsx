import { useCallback, useEffect, useRef } from 'react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { KanbanCard } from '@/components/ui/shadcn-io/kanban';
import {
  CheckCircle,
  Copy,
  Edit,
  Loader2,
  MoreHorizontal,
  Trash2,
  XCircle,
} from 'lucide-react';
import type { TaskWithAttemptStatus } from 'shared/types';
import type { ChildTaskSummary } from '@/hooks/useProjectTasks';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

type Task = TaskWithAttemptStatus;

interface ParentTaskInfo {
  id: string;
  title: string;
}

interface TaskCardProps {
  task: Task;
  index: number;
  status: string;
  onEdit: (task: Task) => void;
  onDelete: (taskId: string) => void;
  onDuplicate?: (task: Task) => void;
  onViewDetails: (task: Task) => void;
  isOpen?: boolean;
  parentTask?: ParentTaskInfo | null;
  onParentClick?: (payload: {
    parent: { taskId: string; title: string };
    sourceTaskId: string;
  }) => void;
  shouldAutoFocusParentPill?: boolean;
  onParentPillFocus?: (taskId: string) => void;
  childTaskSummary?: ChildTaskSummary;
}

export function TaskCard({
  task,
  index,
  status,
  onEdit,
  onDelete,
  onDuplicate,
  onViewDetails,
  isOpen,
  parentTask,
  onParentClick,
  shouldAutoFocusParentPill,
  onParentPillFocus,
  childTaskSummary,
}: TaskCardProps) {
  const handleClick = useCallback(() => {
    onViewDetails(task);
  }, [task, onViewDetails]);

  const localRef = useRef<HTMLDivElement>(null);
  const parentPillRef = useRef<HTMLButtonElement | null>(null);

  const parentTitle = parentTask?.title ?? '';
  const truncatedParentTitle =
    parentTitle.length > 24 ? `${parentTitle.slice(0, 24)}...` : parentTitle;
  const parentLabel = parentTask
    ? `Open parent task ${parentTask.title}`
    : '';
  const childSummary =
    childTaskSummary && childTaskSummary.total > 0 ? childTaskSummary : null;
  const childSummaryLabel = childSummary
    ? `${childSummary.complete} subtasks complete, ${childSummary.inProgress} in progress, ${childSummary.notStarted} not started`
    : '';

  useEffect(() => {
    if (!shouldAutoFocusParentPill || !parentPillRef.current) return;
    parentPillRef.current.focus({ preventScroll: true });
    onParentPillFocus?.(task.id);
  }, [shouldAutoFocusParentPill, onParentPillFocus, task.id]);

  useEffect(() => {
    if (!isOpen || !localRef.current) return;
    const el = localRef.current;
    requestAnimationFrame(() => {
      el.scrollIntoView({
        block: 'center',
        inline: 'nearest',
        behavior: 'smooth',
      });
    });
  }, [isOpen]);

  return (
    <KanbanCard
      key={task.id}
      id={task.id}
      name={task.title}
      index={index}
      parent={status}
      onClick={handleClick}
      isOpen={isOpen}
      forwardedRef={localRef}
      className={cn('relative', childSummary && 'pb-8')}
    >
      <div className="flex flex-1 items-start gap-2 min-w-0">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <h4 className="min-w-0 line-clamp-2 font-light text-sm">
            {task.title}
          </h4>
          {parentTask && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  {onParentClick ? (
                    <Button
                      ref={parentPillRef}
                      type="button"
                      variant="ghost"
                      size="xs"
                      role="link"
                      className={cn(
                        'h-auto gap-1.5 self-start rounded-md border px-2 py-0.5 text-xs font-normal',
                        'bg-muted text-muted-foreground animate-in fade-in-0 animate-pill',
                        'hover:bg-muted/80 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background'
                      )}
                      onClick={(event) => {
                        event.stopPropagation();
                        onParentClick({
                          parent: {
                            taskId: parentTask.id,
                            title: parentTask.title,
                          },
                          sourceTaskId: task.id,
                        });
                      }}
                      onPointerDown={(event) => event.stopPropagation()}
                      onMouseDown={(event) => event.stopPropagation()}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.preventDefault();
                          event.stopPropagation();
                          onParentClick({
                            parent: {
                              taskId: parentTask.id,
                              title: parentTask.title,
                            },
                            sourceTaskId: task.id,
                          });
                        }
                      }}
                      aria-label={parentLabel}
                      data-task-parent-pill={task.id}
                    >
                      {truncatedParentTitle}
                    </Button>
                  ) : (
                    <span
                      className="inline-flex items-center gap-1.5 self-start rounded-md border px-2 py-0.5 bg-muted text-xs text-muted-foreground animate-in fade-in-0 animate-pill focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                      tabIndex={0}
                      onPointerDown={(event) => event.stopPropagation()}
                      onMouseDown={(event) => event.stopPropagation()}
                      onClick={(event) => event.stopPropagation()}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.stopPropagation();
                        }
                      }}
                    >
                      {truncatedParentTitle}
                    </span>
                  )}
                </TooltipTrigger>
                <TooltipContent side="top" align="start">
                  {parentLabel}
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
        </div>
        <div className="flex items-center space-x-1">
          {/* Dev Server Indicator takes priority */}
          {task.has_running_dev_server && (
            <span className="relative flex h-3 w-3 items-center justify-center">
              <span className="absolute inline-flex h-full w-full rounded-full bg-red-500 opacity-75 animate-ping" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-red-500 shadow-[0_0_6px_rgba(239,68,68,0.8)]" />
              <span className="sr-only">Dev server running</span>
            </span>
          )}
          {/* In Progress Spinner */}
          {!task.has_running_dev_server && task.has_in_progress_attempt && (
            <Loader2 className="h-3 w-3 animate-spin text-blue-500" />
          )}
          {/* Merged Indicator */}
          {task.has_merged_attempt && (
            <CheckCircle className="h-3 w-3 text-green-500" />
          )}
          {/* Failed Indicator */}
          {task.last_attempt_failed && !task.has_merged_attempt && (
            <XCircle className="h-3 w-3 text-destructive" />
          )}
          {/* Actions Menu */}
          <div
            onPointerDown={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => e.stopPropagation()}
          >
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0 hover:bg-muted"
                >
                  <MoreHorizontal className="h-3 w-3" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onClick={() => onEdit(task)}>
                  <Edit className="h-4 w-4 mr-2" />
                  Edit
                </DropdownMenuItem>
                {onDuplicate && (
                  <DropdownMenuItem onClick={() => onDuplicate(task)}>
                    <Copy className="h-4 w-4 mr-2" />
                    Duplicate
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem
                  onClick={() => onDelete(task.id)}
                  className="text-destructive"
                >
                  <Trash2 className="h-4 w-4 mr-2" />
                  Delete
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </div>
      {task.description && (
        <p className="flex-1 text-sm text-secondary-foreground break-words">
          {task.description.length > 130
            ? `${task.description.substring(0, 130)}...`
            : task.description}
        </p>
      )}
      {childSummary && (
        <div
          className="absolute bottom-2 right-3 rounded-md border border-border bg-background/95 px-2 py-0.5 text-[11px] font-medium leading-none shadow-sm backdrop-blur"
          aria-label={childSummaryLabel}
        >
          <span className="text-green-500 tabular-nums">
            {childSummary.complete}
          </span>
          <span className="text-muted-foreground mx-1">/</span>
          <span className="text-pink-500 tabular-nums">
            {childSummary.inProgress}
          </span>
          <span className="text-muted-foreground mx-1">/</span>
          <span className="text-muted-foreground tabular-nums">
            {childSummary.notStarted}
          </span>
        </div>
      )}
    </KanbanCard>
  );
}
