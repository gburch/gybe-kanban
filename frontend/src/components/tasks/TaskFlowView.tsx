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
  lane: 'now' | 'next' | 'later';
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
  const rootTasks: Task[] = [];

  // Build initial graph structure
  tasks.forEach((task) => {
    nodes[task.id] = {
      task,
      x: 0,
      y: 0,
      lane: getLane(task),
      children: [],
      parents: [],
      isConvergencePoint: false,
      isBranchPoint: false,
    };
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

  // Find root tasks (no parents)
  tasks.forEach((task) => {
    if (nodes[task.id].parents.length === 0) {
      rootTasks.push(task);
    }
  });

  // Layout algorithm: horizontal flow with swim lanes
  let maxX = 0;
  const laneYPositions: Record<string, number> = {
    now: 0,
    next: 0,
    later: 0,
  };
  const laneXPositions: Record<string, number> = {
    now: 0,
    next: 0,
    later: 0,
  };
  const visited = new Set<string>();

  function layoutNode(taskId: string, depth: number) {
    const node = nodes[taskId];
    if (!node || visited.has(taskId)) return;

    visited.add(taskId);

    const lane = node.lane;

    // For nodes with relationships, use depth-based positioning
    // For isolated nodes, spread them horizontally within their lane
    if (node.children.length > 0 || node.parents.length > 0) {
      node.x = depth * 320;
    } else {
      // Isolated task - use lane-specific x position
      node.x = laneXPositions[lane];
      laneXPositions[lane] += 320; // Move next isolated task in this lane to the right
    }

    node.y = laneYPositions[lane];

    laneYPositions[lane] += 160; // Vertical spacing within lane
    maxX = Math.max(maxX, node.x);

    // Layout children at next depth level
    node.children.forEach((childId) => {
      layoutNode(childId, depth + 1);
    });
  }

  // Layout from roots first (tasks with children)
  rootTasks.forEach((task) => {
    layoutNode(task.id, 0);
  });

  // Layout any remaining unvisited nodes (isolated tasks)
  tasks.forEach((task) => {
    if (!visited.has(task.id)) {
      layoutNode(task.id, 0);
    }
  });

  return { nodes, maxX };
}

function getLane(task: Task): 'now' | 'next' | 'later' {
  const status = task.status.toLowerCase();
  if (status === 'inprogress' || status === 'inreview') return 'now';
  if (status === 'todo') return 'next';
  return 'later';
}

function TaskFlowView({
  tasks,
  onViewTaskDetails,
  selectedTask,
  parentTasksById,
}: TaskFlowViewProps) {
  const { nodes, maxX } = useMemo(
    () => buildFlowLayout(tasks, parentTasksById),
    [tasks, parentTasksById]
  );

  const lanes = ['now', 'next', 'later'] as const;

  // Calculate actual dimensions needed based on card positions
  const CARD_WIDTH = 240;
  const CARD_HEIGHT = 120;
  const LANE_HEIGHT = 300;
  const PADDING = 80;
  const HEADER_HEIGHT = 60;
  const LANE_HEADER_WIDTH = 96; // w-24 in Tailwind

  const containerWidth = Math.max(maxX + CARD_WIDTH + PADDING * 2 + LANE_HEADER_WIDTH, 1200);
  const containerHeight = HEADER_HEIGHT + (LANE_HEIGHT * 3) + PADDING;

  return (
    <div className="w-full h-full bg-background overflow-auto">
      <div className="p-8">
        {/* Flow diagram container with fixed height */}
        <div
          className="relative border rounded-lg bg-muted/20 overflow-visible"
          style={{
            width: containerWidth,
            height: containerHeight,
          }}
        >
          {/* Swim lane headers - positioned on the left */}
          {lanes.map((lane, idx) => (
            <div
              key={lane}
              className="absolute left-0 flex items-center justify-center w-24 border-r bg-background/80"
              style={{
                top: HEADER_HEIGHT + (idx * LANE_HEIGHT),
                height: LANE_HEIGHT,
              }}
            >
              <Badge
                variant="outline"
                className={cn(
                  'text-sm py-1 px-3 font-semibold',
                  lane === 'now' &&
                    'bg-blue-500/10 border-blue-500 text-blue-700 dark:text-blue-300',
                  lane === 'next' &&
                    'bg-purple-500/10 border-purple-500 text-purple-700 dark:text-purple-300',
                  lane === 'later' &&
                    'bg-slate-500/10 border-slate-500 text-slate-700 dark:text-slate-300'
                )}
              >
                {lane.toUpperCase()}
              </Badge>
            </div>
          ))}

          {/* Lane separators */}
          {[1, 2].map((idx) => (
            <div
              key={idx}
              className="absolute right-0 border-t border-border"
              style={{
                left: '96px', // Width of lane header (w-24 = 96px)
                top: HEADER_HEIGHT + idx * LANE_HEIGHT
              }}
            />
          ))}
          {/* SVG for connection lines */}
          <svg
            width={containerWidth}
            height={containerHeight}
            className="absolute top-0 left-0 pointer-events-none"
          >
            {/* Draw connections first (underneath nodes) */}
            {Object.values(nodes).map((node) =>
              node.children.map((childId) => {
                const childNode = nodes[childId];
                if (!childNode) return null;

                const x1 = node.x + CARD_WIDTH + PADDING + LANE_HEADER_WIDTH;
                const y1 = node.y + (CARD_HEIGHT / 2) + getLaneOffset(node.lane, LANE_HEIGHT, HEADER_HEIGHT) + PADDING;
                const x2 = childNode.x + PADDING + LANE_HEADER_WIDTH;
                const y2 = childNode.y + (CARD_HEIGHT / 2) + getLaneOffset(childNode.lane, LANE_HEIGHT, HEADER_HEIGHT) + PADDING;

                // Curved path
                const midX = (x1 + x2) / 2;
                const path = `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`;

                return (
                  <g key={`${node.task.id}-${childId}`}>
                    <path
                      d={path}
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      className={cn(
                        'text-muted-foreground/40',
                        childNode.isConvergencePoint &&
                          'stroke-amber-500 stroke-[3]'
                      )}
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
                <polygon
                  points="0 0, 10 3, 0 6"
                  className="fill-muted-foreground/40"
                />
              </marker>
              <marker
                id="arrowhead-critical"
                markerWidth="12"
                markerHeight="12"
                refX="10"
                refY="3"
                orient="auto"
              >
                <polygon
                  points="0 0, 12 3, 0 6"
                  className="fill-amber-500"
                />
              </marker>
            </defs>
          </svg>

          {/* Task nodes */}
          {Object.values(nodes).map((node) => (
            <div
              key={node.task.id}
              className={cn(
                'absolute w-[240px] cursor-pointer transition-all duration-200',
                'hover:scale-105 hover:z-10'
              )}
              style={{
                left: `${node.x + PADDING + LANE_HEADER_WIDTH}px`,
                top: `${node.y + getLaneOffset(node.lane, LANE_HEIGHT, HEADER_HEIGHT) + PADDING}px`,
              }}
              onClick={() => onViewTaskDetails(node.task)}
            >
              <div
                className={cn(
                  'rounded-lg border-2 bg-card p-4 shadow-md',
                  selectedTask?.id === node.task.id &&
                    'ring-2 ring-primary ring-offset-2',
                  node.isConvergencePoint &&
                    'border-amber-500 shadow-amber-500/20 shadow-lg',
                  !node.isConvergencePoint && 'border-border'
                )}
              >
                {/* Convergence indicator */}
                {node.isConvergencePoint && (
                  <div className="flex items-center gap-1 mb-2 text-xs font-semibold text-amber-600 dark:text-amber-400">
                    <GitMerge className="h-3 w-3" />
                    <span>Critical Path</span>
                  </div>
                )}

                {/* Branch indicator */}
                {node.isBranchPoint && (
                  <div className="flex items-center gap-1 mb-2 text-xs font-semibold text-blue-600 dark:text-blue-400">
                    <GitBranch className="h-3 w-3" />
                    <span>Branches</span>
                  </div>
                )}

                {/* Task status icon */}
                <div className="flex items-start gap-2 mb-2">
                  {getStatusIcon(node.task)}
                  <div className="flex-1 min-w-0">
                    <h3 className="font-medium text-sm line-clamp-2">
                      {node.task.title}
                    </h3>
                  </div>
                </div>

                {/* Task metadata */}
                <div className="flex items-center justify-between mt-2">
                  <Badge variant="outline" className="text-xs">
                    {getStatusLabel(node.task.status)}
                  </Badge>
                  {node.children.length > 0 && (
                    <span className="text-xs text-muted-foreground">
                      {node.children.length} child
                      {node.children.length > 1 ? 'ren' : ''}
                    </span>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>

        {/* Legend - outside the flow container */}
        <div className="mt-8 flex flex-wrap items-center gap-6 text-sm text-muted-foreground">
          <div className="flex items-center gap-2">
            <div className="w-8 h-0.5 bg-muted-foreground/40" />
            <span>Dependency</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-8 h-0.5 bg-amber-500" />
            <span>Critical Path (Multiple dependencies merge)</span>
          </div>
          <div className="flex items-center gap-2">
            <GitMerge className="h-4 w-4 text-amber-500" />
            <span>Convergence Point</span>
          </div>
          <div className="flex items-center gap-2">
            <GitBranch className="h-4 w-4 text-blue-500" />
            <span>Branch Point</span>
          </div>
        </div>
      </div>
    </div>
  );
}

function getLaneOffset(lane: 'now' | 'next' | 'later', laneHeight: number, headerHeight: number): number {
  const laneIndex = { now: 0, next: 1, later: 2 }[lane];
  return headerHeight + (laneIndex * laneHeight);
}

function getStatusIcon(task: Task) {
  const status = task.status.toLowerCase();
  if (status === 'done') {
    return <CheckCircle className="h-4 w-4 text-green-500 flex-shrink-0" />;
  }
  if (status === 'inprogress') {
    return <Loader2 className="h-4 w-4 text-blue-500 animate-spin flex-shrink-0" />;
  }
  if (status === 'cancelled') {
    return <Circle className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
  }
  return <Circle className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
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