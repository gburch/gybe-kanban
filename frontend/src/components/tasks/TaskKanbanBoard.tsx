import { memo } from 'react';
import {
  type DragEndEvent,
  KanbanBoard,
  KanbanCards,
  KanbanHeader,
  KanbanProvider,
} from '@/components/ui/shadcn-io/kanban';
import { TaskCard } from './TaskCard';
import type { TaskStatus, TaskWithAttemptStatus } from 'shared/types';
import type {
  ChildTaskSummary,
  ParentTaskSummary,
} from '@/hooks/useProjectTasks';
// import { useParams } from 'react-router-dom';

import { statusBoardColors, statusLabels } from '@/utils/status-labels';

type Task = TaskWithAttemptStatus;

interface TaskKanbanBoardProps {
  groupedTasks: Record<TaskStatus, Task[]>;
  onDragEnd: (event: DragEndEvent) => void;
  onEditTask: (task: Task) => void;
  onDeleteTask: (taskId: string) => void;
  onDuplicateTask?: (task: Task) => void;
  onViewTaskDetails: (task: Task) => void;
  selectedTask?: Task;
  onCreateTask?: () => void;
  parentTasksById?: Record<string, ParentTaskSummary | null>;
  childTaskSummaryById?: Record<string, ChildTaskSummary>;
  onParentClick?: (payload: {
    parent: { taskId: string; title: string };
    sourceTaskId: string;
  }) => void;
  focusParentPillId?: string | null;
  onParentPillFocus?: (taskId: string) => void;
}

function TaskKanbanBoard({
  groupedTasks,
  onDragEnd,
  onEditTask,
  onDeleteTask,
  onDuplicateTask,
  onViewTaskDetails,
  selectedTask,
  onCreateTask,
  parentTasksById = {},
  childTaskSummaryById = {},
  onParentClick,
  focusParentPillId,
  onParentPillFocus,
}: TaskKanbanBoardProps) {
  return (
    <KanbanProvider onDragEnd={onDragEnd}>
      {Object.entries(groupedTasks).map(([status, statusTasks]) => (
        <KanbanBoard key={status} id={status as TaskStatus}>
          <KanbanHeader
            name={statusLabels[status as TaskStatus]}
            color={statusBoardColors[status as TaskStatus]}
            onAddTask={onCreateTask}
          />
          <KanbanCards>
            {statusTasks.map((task, index) => {
              const parentSummary = parentTasksById?.[task.id] ?? null;
              const childSummary = childTaskSummaryById?.[task.id];

              return (
                <TaskCard
                  key={task.id}
                  task={task}
                  index={index}
                  status={status}
                  onEdit={onEditTask}
                  onDelete={onDeleteTask}
                  onDuplicate={onDuplicateTask}
                  onViewDetails={onViewTaskDetails}
                  isOpen={selectedTask?.id === task.id}
                  parentTask={
                    parentSummary
                      ? { id: parentSummary.id, title: parentSummary.title }
                      : null
                  }
                  childTaskSummary={childSummary}
                  onParentClick={onParentClick}
                  shouldAutoFocusParentPill={focusParentPillId === task.id}
                  onParentPillFocus={onParentPillFocus}
                />
              );
            })}
          </KanbanCards>
        </KanbanBoard>
      ))}
    </KanbanProvider>
  );
}

export default memo(TaskKanbanBoard);
