import { memo, useMemo } from 'react';
import type { TaskWithAttemptStatus } from 'shared/types';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import {
  CheckCircle,
  Circle,
  Loader2,
  GitBranch,
  GitMerge,
} from 'lucide-react';

type Task = TaskWithAttemptStatus;

interface ParentTaskSummary {
  id: string;
  title: string;
}

interface TaskFlowViewProps {
  tasks: Task[];
  tasksById: Record<string, Task>;
  onViewTaskDetails: (task: Task) => void;
  selectedTask?: Task;
  parentTasksById?: Record<string, ParentTaskSummary | null>;
}

interface FlowNode {
  task: Task;
  x: number;
  y: number;
  column: number; // 0 = now, 1 = next, 2 = later
  children: string[];
  parents: string[];
  isConvergencePoint: boolean;
  isBranchPoint: boolean;
}

// Helper to build the task graph and layout
function buildFlowLayout(
  tasks: Task[],
  parentTasksById?: Record<string, ParentTaskSummary | null>
) {
  const nodes: Record<string, FlowNode> = {};
  const columns: Task[][] = [[], [], []]; // now, next, later

  // Build initial graph structure
  tasks.forEach((task) => {
    const columnIndex = getColumnIndex(task);
    nodes[task.id] = {
      task,
      x: 0,
      y: 0,
      column: columnIndex,
      children: [],
      parents: [],
      isConvergencePoint: false,
      isBranchPoint: false,
    };
    columns[columnIndex].push(task);
  });

  // Find parent-child relationships using parentTasksById
  if (parentTasksById) {
    tasks.forEach((task) => {
      const parentSummary = parentTasksById[task.id];
      if (parentSummary && parentSummary.id) {
        const parentId = parentSummary.id;
        if (nodes[parentId] && nodes[task.id]) {
          nodes[parentId].children.push(task.id);
          nodes[task.id].parents.push(parentId);
        }
      }
    });
  }

  // Identify convergence and branch points
  Object.values(nodes).forEach((node) => {
    node.isConvergencePoint = node.parents.length > 1;
    node.isBranchPoint = node.children.length > 1;
  });

  // Layout: Position cards in a grid within each column
  const COLUMN_WIDTH = 400;
  const CARD_WIDTH = 280;
  const CARD_HEIGHT = 140;
  const VERTICAL_GAP = 20;
  const HORIZONTAL_PADDING = 60;
  const LANE_HEADER_WIDTH = 120;

  columns.forEach((columnTasks, colIndex) => {
    columnTasks.forEach((task, taskIndex) => {
      const node = nodes[task.id];
      node.x = LANE_HEADER_WIDTH + HORIZONTAL_PADDING + (colIndex * COLUMN_WIDTH);
      node.y = HORIZONTAL_PADDING + (taskIndex * (CARD_HEIGHT + VERTICAL_GAP));
    });
  });

  const maxY = Math.max(
    ...columns.map(
      (col) =>
        HORIZONTAL_PADDING +
        col.length * (CARD_HEIGHT + VERTICAL_GAP)
    ),
    600
  );

  const totalWidth = LANE_HEADER_WIDTH + HORIZONTAL_PADDING * 2 + (3 * COLUMN_WIDTH) + CARD_WIDTH;

  return { nodes, totalWidth, totalHeight: maxY + HORIZONTAL_PADDING };
}

function getColumnIndex(task: Task): number {
  const status = task.status.toLowerCase();
  if (status === 'inprogress' || status === 'inreview') return 0; // NOW
  if (status === 'todo') return 1; // NEXT
  return 2; // LATER (done, cancelled)
}

function TaskFlowView({
  tasks,
  onViewTaskDetails,
  selectedTask,
  parentTasksById,
}: TaskFlowViewProps) {
  const { nodes, totalWidth, totalHeight } = useMemo(
    () => buildFlowLayout(tasks, parentTasksById),
    [tasks, parentTasksById]
  );

  const columns = ['NOW', 'NEXT', 'LATER'] as const;
  const COLUMN_WIDTH = 400;
  const LANE_HEADER_WIDTH = 120;
  const CARD_WIDTH = 280;

  return (
    <div className="w-full h-full bg-background overflow-auto">
      <div className="p-8">
        {/* Flow diagram container */}
        <div
          className="relative rounded-lg border bg-slate-900/50"
          style={{
            width: totalWidth,
            height: totalHeight,
            minHeight: '600px',
          }}
        >
          {/* Column headers and vertical separators */}
          {columns.map((label, idx) => (
            <div key={label}>
              {/* Column header */}
              <div
                className="absolute top-0 flex items-center justify-center"
                style={{
                  left: LANE_HEADER_WIDTH + 60 + (idx * COLUMN_WIDTH),
                  width: COLUMN_WIDTH,
                  height: 60,
                }}
              >
                <Badge
                  variant="outline"
                  className={cn(
                    'text-sm py-1.5 px-4 font-semibold',
                    idx === 0 &&
                      'bg-blue-500/10 border-blue-500 text-blue-300',
                    idx === 1 &&
                      'bg-purple-500/10 border-purple-500 text-purple-300',
                    idx === 2 &&
                      'bg-slate-500/10 border-slate-500 text-slate-300'
                  )}
                >
                  {label}
                </Badge>
              </div>

              {/* Vertical separator line */}
              {idx < 2 && (
                <div
                  className="absolute top-16 bottom-0 w-px bg-slate-700/50"
                  style={{
                    left: LANE_HEADER_WIDTH + 60 + ((idx + 1) * COLUMN_WIDTH) - COLUMN_WIDTH / 2 + CARD_WIDTH / 2,
                  }}
                />
              )}
            </div>
          ))}

          {/* SVG for connection lines */}
          <svg
            width={totalWidth}
            height={totalHeight}
            className="absolute top-0 left-0 pointer-events-none"
            style={{ zIndex: 1 }}
          >
            {/* Draw connections */}
            {Object.values(nodes).map((node) =>
              node.children.map((childId) => {
                const childNode = nodes[childId];
                if (!childNode) return null;

                const x1 = node.x + CARD_WIDTH;
                const y1 = node.y + 70; // Middle of card
                const x2 = childNode.x;
                const y2 = childNode.y + 70;

                // Curved path
                const midX = (x1 + x2) / 2;
                const path = `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`;

                return (
                  <g key={`${node.task.id}-${childId}`}>
                    <path
                      d={path}
                      fill="none"
                      stroke={childNode.isConvergencePoint ? '#f59e0b' : '#64748b'}
                      strokeWidth={childNode.isConvergencePoint ? 3 : 2}
                      opacity={0.6}
                      markerEnd={
                        childNode.isConvergencePoint
                          ? 'url(#arrowhead-critical)'
                          : 'url(#arrowhead)'
                      }
                    />
                  </g>
                );
              })
            )}

            {/* Arrow markers */}
            <defs>
              <marker
                id="arrowhead"
                markerWidth="10"
                markerHeight="10"
                refX="9"
                refY="3"
                orient="auto"
              >
                <polygon points="0 0, 10 3, 0 6" fill="#64748b" opacity="0.6" />
              </marker>
              <marker
                id="arrowhead-critical"
                markerWidth="12"
                markerHeight="12"
                refX="10"
                refY="3"
                orient="auto"
              >
                <polygon points="0 0, 12 3, 0 6" fill="#f59e0b" />
              </marker>
            </defs>
          </svg>

          {/* Task nodes */}
          {Object.values(nodes).map((node) => (
            <div
              key={node.task.id}
              className={cn(
                'absolute cursor-pointer transition-all duration-200',
                'hover:scale-105 hover:z-20'
              )}
              style={{
                left: `${node.x}px`,
                top: `${node.y}px`,
                width: `${CARD_WIDTH}px`,
                zIndex: 10,
              }}
              onClick={() => onViewTaskDetails(node.task)}
            >
              <div
                className={cn(
                  'rounded-lg border-2 bg-slate-800 p-4 shadow-lg h-full',
                  selectedTask?.id === node.task.id &&
                    'ring-2 ring-blue-500 ring-offset-2 ring-offset-slate-900',
                  node.isConvergencePoint &&
                    'border-amber-500 shadow-amber-500/30',
                  !node.isConvergencePoint && 'border-slate-700'
                )}
              >
                {/* Convergence indicator */}
                {node.isConvergencePoint && (
                  <div className="flex items-center gap-1 mb-2 text-xs font-semibold text-amber-400">
                    <GitMerge className="h-3 w-3" />
                    <span>Critical Path</span>
                  </div>
                )}

                {/* Branch indicator */}
                {node.isBranchPoint && (
                  <div className="flex items-center gap-1 mb-2 text-xs font-semibold text-blue-400">
                    <GitBranch className="h-3 w-3" />
                    <span>Branches</span>
                  </div>
                )}

                {/* Task content */}
                <div className="flex items-start gap-2 mb-2">
                  {getStatusIcon(node.task)}
                  <div className="flex-1 min-w-0">
                    <h3 className="font-medium text-sm line-clamp-2 text-slate-100">
                      {node.task.title}
                    </h3>
                  </div>
                </div>

                {/* Task metadata */}
                <div className="flex items-center justify-between mt-3">
                  <Badge variant="outline" className="text-xs bg-slate-900/50 border-slate-600 text-slate-300">
                    {getStatusLabel(node.task.status)}
                  </Badge>
                  {node.children.length > 0 && (
                    <span className="text-xs text-slate-400">
                      {node.children.length} child{node.children.length > 1 ? 'ren' : ''}
                    </span>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>

        {/* Legend */}
        <div className="mt-8 flex flex-wrap items-center gap-6 text-sm text-muted-foreground">
          <div className="flex items-center gap-2">
            <div className="w-8 h-0.5 bg-slate-500" />
            <span>Dependency</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-8 h-0.5 bg-amber-500" />
            <span>Critical Path</span>
          </div>
          <div className="flex items-center gap-2">
            <GitMerge className="h-4 w-4 text-amber-500" />
            <span>Convergence Point</span>
          </div>
          <div className="flex items-center gap-2">
            <GitBranch className="h-4 w-4 text-blue-400" />
            <span>Branch Point</span>
          </div>
        </div>
      </div>
    </div>
  );
}

function getStatusIcon(task: Task) {
  const status = task.status.toLowerCase();
  if (status === 'done') {
    return <CheckCircle className="h-4 w-4 text-green-400 flex-shrink-0" />;
  }
  if (status === 'inprogress') {
    return <Loader2 className="h-4 w-4 text-blue-400 animate-spin flex-shrink-0" />;
  }
  if (status === 'cancelled') {
    return <Circle className="h-4 w-4 text-slate-500 flex-shrink-0" />;
  }
  return <Circle className="h-4 w-4 text-slate-400 flex-shrink-0" />;
}

function getStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    todo: 'To Do',
    inprogress: 'In Progress',
    inreview: 'In Review',
    done: 'Done',
    cancelled: 'Cancelled',
  };
  return labels[status.toLowerCase()] || status;
}

export default memo(TaskFlowView);