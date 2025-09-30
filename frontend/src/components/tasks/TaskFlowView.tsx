import { memo, useMemo } from 'react';
import type { TaskWithAttemptStatus } from 'shared/types';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent } from '@/components/ui/card';
import {
  CheckCircle,
  Circle,
  Loader2,
  GitBranch,
  GitMerge,
} from 'lucide-react';
import dagre from 'dagre';

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

// Detect cycles in the graph using DFS
function detectCycles(
  nodeId: string,
  nodes: Record<string, FlowNode>,
  visited: Set<string>,
  recursionStack: Set<string>,
  cycles: string[][]
): boolean {
  visited.add(nodeId);
  recursionStack.add(nodeId);

  const node = nodes[nodeId];
  if (!node) return false;

  for (const childId of node.children) {
    if (!visited.has(childId)) {
      if (detectCycles(childId, nodes, visited, recursionStack, cycles)) {
        return true;
      }
    } else if (recursionStack.has(childId)) {
      // Cycle detected
      cycles.push([nodeId, childId]);
      return true;
    }
  }

  recursionStack.delete(nodeId);
  return false;
}

// Helper to build the task graph and layout using Dagre
function buildFlowLayout(
  tasks: Task[],
  parentTasksById?: Record<string, ParentTaskSummary | null>
) {
  const nodes: Record<string, FlowNode> = {};

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
  });

  // Find parent-child relationships
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

  // Check for cycles
  const visited = new Set<string>();
  const cycles: string[][] = [];
  Object.keys(nodes).forEach((nodeId) => {
    if (!visited.has(nodeId)) {
      detectCycles(nodeId, nodes, visited, new Set(), cycles);
    }
  });

  if (cycles.length > 0) {
    console.warn('Circular dependencies detected:', cycles);
    // Remove cycle edges (keep only parent→child direction)
    cycles.forEach(([parentId, childId]) => {
      const parent = nodes[parentId];
      if (parent) {
        parent.children = parent.children.filter(id => id !== childId);
      }
    });
  }

  // Identify convergence and branch points
  Object.values(nodes).forEach((node) => {
    node.isConvergencePoint = node.parents.length > 1;
    node.isBranchPoint = node.children.length > 1;
  });

  // Create Dagre graph for hierarchical layout
  const g = new dagre.graphlib.Graph();
  g.setGraph({
    rankdir: 'LR', // Left to right (temporal flow)
    nodesep: 80,   // Vertical spacing between nodes
    ranksep: 200,  // Horizontal spacing between ranks
    marginx: 60,
    marginy: 60,
  });
  g.setDefaultEdgeLabel(() => ({}));

  // Constants
  const CARD_WIDTH = 320;
  const CARD_HEIGHT = 140;

  // Add nodes to Dagre
  Object.values(nodes).forEach((node) => {
    g.setNode(node.task.id, {
      width: CARD_WIDTH,
      height: CARD_HEIGHT,
      column: node.column,
    });
  });

  // Add edges to Dagre
  Object.values(nodes).forEach((node) => {
    node.children.forEach((childId) => {
      g.setEdge(node.task.id, childId);
    });
  });

  // Run Dagre layout
  dagre.layout(g);

  // Update node positions from Dagre
  g.nodes().forEach((nodeId) => {
    const dagreNode = g.node(nodeId);
    const node = nodes[nodeId];
    if (node && dagreNode) {
      // Dagre positions nodes at their center
      node.x = dagreNode.x - CARD_WIDTH / 2;
      node.y = dagreNode.y - CARD_HEIGHT / 2;
    }
  });

  // Calculate bounds
  const allNodes = Object.values(nodes);
  const maxX = Math.max(...allNodes.map(n => n.x + CARD_WIDTH), 1200);
  const maxY = Math.max(...allNodes.map(n => n.y + CARD_HEIGHT), 800);

  return {
    nodes,
    totalWidth: maxX + 60,
    totalHeight: maxY + 60,
  };
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

  const CARD_WIDTH = 320;

  return (
    <div className="w-full h-full bg-background overflow-auto">
      <div className="p-8">
        {/* Flow diagram container */}
        <div
          className="relative rounded-lg border border-border bg-zinc-950/50"
          style={{
            width: totalWidth,
            height: totalHeight,
            minHeight: '600px',
          }}
        >

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
                      stroke={childNode.isConvergencePoint ? '#fbbf24' : '#71717a'}
                      strokeWidth={childNode.isConvergencePoint ? 3 : 2}
                      opacity={childNode.isConvergencePoint ? 0.8 : 0.5}
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
                <polygon points="0 0, 10 3, 0 6" fill="#71717a" opacity="0.5" />
              </marker>
              <marker
                id="arrowhead-critical"
                markerWidth="12"
                markerHeight="12"
                refX="10"
                refY="3"
                orient="auto"
              >
                <polygon points="0 0, 12 3, 0 6" fill="#fbbf24" opacity="0.8" />
              </marker>
            </defs>
          </svg>

          {/* Task nodes */}
          {Object.values(nodes).map((node) => (
            <Card
              key={node.task.id}
              className={cn(
                'absolute cursor-pointer transition-all duration-200',
                'hover:scale-105 hover:z-20',
                'border-2 shadow-lg',
                selectedTask?.id === node.task.id &&
                  'ring-2 ring-primary ring-offset-2 ring-offset-background',
                node.isConvergencePoint &&
                  'border-amber-500/70 shadow-amber-500/20 bg-amber-950/20',
                !node.isConvergencePoint && 'border-zinc-700/70 bg-zinc-900/80'
              )}
              style={{
                left: `${node.x}px`,
                top: `${node.y}px`,
                width: `${CARD_WIDTH}px`,
                zIndex: 10,
              }}
              onClick={() => onViewTaskDetails(node.task)}
            >
              <CardContent className="p-4">
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
                    <h3 className="font-medium text-sm line-clamp-2 text-foreground">
                      {node.task.title}
                    </h3>
                  </div>
                </div>

                {/* Task metadata */}
                <div className="flex items-center justify-between mt-3">
                  <Badge variant="outline" className="text-xs">
                    {getStatusLabel(node.task.status)}
                  </Badge>
                  {node.children.length > 0 && (
                    <span className="text-xs text-muted-foreground">
                      {node.children.length} child{node.children.length > 1 ? 'ren' : ''}
                    </span>
                  )}
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        {/* Legend */}
        <div className="mt-8 flex flex-wrap items-center gap-6 text-sm text-muted-foreground">
          <div className="flex items-center gap-2">
            <div className="w-8 h-0.5 bg-zinc-500" />
            <span>Dependency</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-8 h-0.5 bg-amber-400" />
            <span>Critical Path</span>
          </div>
          <div className="flex items-center gap-2">
            <GitMerge className="h-4 w-4 text-amber-400" />
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