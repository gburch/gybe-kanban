import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TaskCard } from '@/components/tasks/TaskCard';
import type { TaskWithAttemptStatus } from 'shared/types';
import { DndContext } from '@dnd-kit/core';
import type { ReactNode } from 'react';

const baseTask = (): TaskWithAttemptStatus => ({
  id: 'task-1',
  project_id: 'project-1',
  title: 'Child task title',
  description: 'Child description',
  status: 'todo',
  parent_task_attempt: null,
  parent_task_id: null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  has_in_progress_attempt: false,
  has_merged_attempt: false,
  last_attempt_failed: false,
  has_running_dev_server: false,
  executor: 'executor',
});

const renderWithDnd = (ui: ReactNode) =>
  render(<DndContext onDragEnd={() => {}}>{ui}</DndContext>);

describe('TaskCard', () => {
  it('renders parent pill with truncated title and tooltip trigger', () => {
    const parentTitle = 'This parent task title is definitely longer than twenty four characters';
    const expectedTruncated = `${parentTitle.slice(0, 24)}...`;

    renderWithDnd(
      <TaskCard
        task={baseTask()}
        index={0}
        status="todo"
        onEdit={() => {}}
        onDelete={() => {}}
        onViewDetails={() => {}}
        parentTask={{ id: 'parent-1', title: parentTitle }}
      />
    );

    expect(screen.getByText(expectedTruncated)).toBeInTheDocument();
    expect(
      screen.queryByRole('link', {
        name: `Open parent task ${parentTitle}`,
      })
    ).toBeNull();
  });

  it('does not render parent pill when no parent metadata is provided', () => {
    renderWithDnd(
      <TaskCard
        task={baseTask()}
        index={0}
        status="todo"
        onEdit={() => {}}
        onDelete={() => {}}
        onViewDetails={() => {}}
      />
    );

    expect(screen.queryByRole('link', { name: /Open parent task/i })).toBeNull();
    expect(screen.getByText('Child task title')).toBeInTheDocument();
  });

  it('invokes onParentClick with id and title when pill is clicked', async () => {
    const user = userEvent.setup();
    const parentTitle = 'Parent navigation target';
    const onParentClick = vi.fn();

    renderWithDnd(
      <TaskCard
        task={baseTask()}
        index={0}
        status="todo"
        onEdit={() => {}}
        onDelete={() => {}}
        onViewDetails={() => {}}
        parentTask={{ id: 'parent-123', title: parentTitle }}
        onParentClick={onParentClick}
      />
    );

    await user.click(
      screen.getByRole('link', { name: `Open parent task ${parentTitle}` })
    );

    expect(onParentClick).toHaveBeenCalledWith({
      parent: {
        taskId: 'parent-123',
        title: parentTitle,
      },
      sourceTaskId: 'task-1',
    });
  });

  it('invokes onParentClick when activated with keyboard', async () => {
    const user = userEvent.setup();
    const parentTitle = 'Keyboard parent trigger';
    const onParentClick = vi.fn();

    renderWithDnd(
      <TaskCard
        task={baseTask()}
        index={0}
        status="todo"
        onEdit={() => {}}
        onDelete={() => {}}
        onViewDetails={() => {}}
        parentTask={{ id: 'parent-keyboard', title: parentTitle }}
        onParentClick={onParentClick}
      />
    );

    const pill = screen.getByRole('link', {
      name: `Open parent task ${parentTitle}`,
    });

    pill.focus();
    await user.keyboard('{Enter}');
    await user.keyboard(' ');

    expect(onParentClick).toHaveBeenCalledTimes(2);
    expect(onParentClick).toHaveBeenLastCalledWith({
      parent: {
        taskId: 'parent-keyboard',
        title: parentTitle,
      },
      sourceTaskId: 'task-1',
    });
  });

  it('renders child task progress badge when summary data exists', () => {
    renderWithDnd(
      <TaskCard
        task={baseTask()}
        index={0}
        status="todo"
        onEdit={() => {}}
        onDelete={() => {}}
        onViewDetails={() => {}}
        childTaskSummary={{
          complete: 1,
          inProgress: 2,
          notStarted: 0,
          total: 3,
        }}
      />
    );

    expect(screen.getByText('1/2/0')).toBeInTheDocument();
    expect(
      screen.getByLabelText('1 subtasks complete, 2 in progress, 0 not started')
    ).toBeInTheDocument();
  });
});
