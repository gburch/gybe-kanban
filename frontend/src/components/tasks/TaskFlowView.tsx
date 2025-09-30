import { memo, useMemo, useEffect, useRef, useState, useCallback } from 'react';
import type { TaskWithAttemptStatus } from 'shared/types';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import {
  CheckCircle,
  Circle,
  Loader2,
  GitBranch,
  GitMerge,
  ArrowLeft,
  ArrowRight,
  ZoomIn,
  ZoomOut,
  Maximize2,
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
    rankdir: 'LR', // Left to right - we'll reverse edge direction to get children left of parents
    nodesep: 60,   // Vertical spacing between nodes (reduced from 100)
    ranksep: 280,  // Horizontal spacing between ranks (reduced from 350)
    marginx: 40,   // Reduced margin (from 80)
    marginy: 40,   // Reduced margin (from 80)
  });
  g.setDefaultEdgeLabel(() => ({}));

  // Constants
  const CARD_WIDTH = 300;   // Reduced from 320
  const CARD_HEIGHT = 130;  // Reduced from 140

  // Add nodes to Dagre with rank constraints based on status
  // Rank determines horizontal position: lower rank = further left
  Object.values(nodes).forEach((node) => {
    const status = node.task.status.toLowerCase();
    let rank: number | undefined;

    // Set rank based on status to align tasks horizontally by status
    // This overrides the graph structure to group by status
    if (status === 'done' || status === 'cancelled') {
      rank = 0; // Leftmost - completed tasks
    } else if (status === 'inprogress' || status === 'inreview') {
      rank = 2; // Middle-right - active tasks
    } else if (status === 'todo') {
      rank = 3; // Rightmost - future tasks
    }

    g.setNode(node.task.id, {
      width: CARD_WIDTH,
      height: CARD_HEIGHT,
      rank, // Set rank to control horizontal position
    });
  });

  // Add edges to Dagre - REVERSED so children appear left of parents
  // In LR mode: edge(A, B) means A is left of B
  // So we do edge(child, parent) to put children on the left
  Object.values(nodes).forEach((node) => {
    node.children.forEach((childId) => {
      g.setEdge(childId, node.task.id); // Reversed: child → parent
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

  // Find leftmost in-progress or in-review task for initial scroll position
  // We want to focus on active work, not future todos
  const activeNodes = allNodes.filter(n =>
    n.task.status.toLowerCase() === 'inprogress' ||
    n.task.status.toLowerCase() === 'inreview'
  );
  const focusX = activeNodes.length > 0
    ? Math.min(...activeNodes.map(n => n.x)) // LEFTMOST active task
    : 0;

  return {
    nodes,
    totalWidth: maxX + 80,
    totalHeight: maxY + 80,
    focusX, // X position to scroll to
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
  const { nodes, totalWidth, totalHeight, focusX } = useMemo(
    () => buildFlowLayout(tasks, parentTasksById),
    [tasks, parentTasksById]
  );

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const flowContainerRef = useRef<HTMLDivElement>(null);
  const minimapRef = useRef<HTMLCanvasElement>(null);
  const [zoom, setZoom] = useState(1);
  const [isMinimapDragging, setIsMinimapDragging] = useState(false);
  const CARD_WIDTH = 300;

  // Zoom controls
  const handleZoomIn = useCallback(() => {
    setZoom(prev => Math.min(prev + 0.2, 2));
  }, []);

  const handleZoomOut = useCallback(() => {
    setZoom(prev => Math.max(prev - 0.2, 0.5));
  }, []);

  const handleZoomReset = useCallback(() => {
    setZoom(1);
  }, []);

  // Minimap interaction handlers
  const handleMinimapClick = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!minimapRef.current || !scrollContainerRef.current) return;

    const canvas = minimapRef.current;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    const minimapWidth = 200;
    const minimapHeight = 150;
    const scale = Math.min(minimapWidth / totalWidth, minimapHeight / totalHeight);

    const scrollX = (x / scale) * zoom - scrollContainerRef.current.clientWidth / 2;
    const scrollY = (y / scale) * zoom - scrollContainerRef.current.clientHeight / 2;

    scrollContainerRef.current.scrollTo({
      left: Math.max(0, scrollX),
      top: Math.max(0, scrollY),
      behavior: 'smooth',
    });
  }, [totalWidth, totalHeight, zoom]);

  const handleMinimapMouseDown = useCallback(() => {
    setIsMinimapDragging(true);
  }, []);

  const handleMinimapMouseUp = useCallback(() => {
    setIsMinimapDragging(false);
  }, []);

  const handleMinimapMouseMove = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!isMinimapDragging) return;
    handleMinimapClick(e);
  }, [isMinimapDragging, handleMinimapClick]);

  // Auto-scroll to focus on active tasks
  useEffect(() => {
    if (scrollContainerRef.current && focusX > 0) {
      // Scroll to show the leftmost active task, with some padding
      const container = scrollContainerRef.current;
      const scrollTo = focusX * zoom - container.clientWidth / 2 + (CARD_WIDTH * zoom) / 2;
      container.scrollTo({
        left: Math.max(0, scrollTo),
        behavior: 'smooth',
      });
    }
  }, [focusX, CARD_WIDTH, zoom]);

  // Draw minimap
  useEffect(() => {
    if (!minimapRef.current || !scrollContainerRef.current) return;

    const drawMinimap = () => {
      const canvas = minimapRef.current;
      const ctx = canvas?.getContext('2d');
      if (!ctx || !canvas) return;

      const minimapWidth = 200;
      const minimapHeight = 150;
      const scale = Math.min(minimapWidth / totalWidth, minimapHeight / totalHeight);

      canvas.width = minimapWidth;
      canvas.height = minimapHeight;

      // Clear canvas - use gray background
      ctx.fillStyle = '#18181b'; // zinc-900
      ctx.fillRect(0, 0, minimapWidth, minimapHeight);

      // Draw nodes
      Object.values(nodes).forEach(node => {
        const x = node.x * scale;
        const y = node.y * scale;
        const w = CARD_WIDTH * scale;
        const h = 130 * scale; // CARD_HEIGHT

        const status = node.task.status.toLowerCase();
        if (status === 'done' || status === 'cancelled') {
          ctx.fillStyle = 'rgba(34, 197, 94, 0.4)'; // green-500
        } else if (status === 'inprogress' || status === 'inreview') {
          ctx.fillStyle = 'rgba(59, 130, 246, 0.7)'; // blue-500
        } else {
          ctx.fillStyle = 'rgba(161, 161, 170, 0.4)'; // zinc-400
        }
        ctx.fillRect(x, y, w, h);
      });

      // Draw viewport rectangle
      const container = scrollContainerRef.current;
      if (!container) return;

      const viewportX = (container.scrollLeft / zoom) * scale;
      const viewportY = (container.scrollTop / zoom) * scale;
      const viewportW = (container.clientWidth / zoom) * scale;
      const viewportH = (container.clientHeight / zoom) * scale;

      ctx.strokeStyle = 'rgba(59, 130, 246, 1)'; // blue-500
      ctx.lineWidth = 2;
      ctx.strokeRect(viewportX, viewportY, viewportW, viewportH);
    };

    drawMinimap();

    // Redraw on scroll
    const container = scrollContainerRef.current;
    if (container) {
      container.addEventListener('scroll', drawMinimap);
      return () => container.removeEventListener('scroll', drawMinimap);
    }
  }, [nodes, totalWidth, totalHeight, zoom, CARD_WIDTH]);

  return (
    <div ref={scrollContainerRef} className="w-full h-full bg-background overflow-auto relative">
      {/* Legend - Sticky at top */}
      <div className="sticky top-0 z-40 bg-background/95 backdrop-blur-sm border-b border-border/50 px-8 py-3">
        <div className="flex flex-wrap items-center gap-6 text-sm text-muted-foreground">
          <div className="flex items-center gap-2">
            <div className="w-8 h-0.5 bg-border" />
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
          <div className="flex items-center gap-2 ml-auto bg-background/80 backdrop-blur-sm px-3 py-1.5 rounded-md border border-border/50">
            <ArrowLeft className="w-3 h-3" />
            <span className="text-xs">Completed</span>
            <span className="mx-2 opacity-50">|</span>
            <span className="text-xs">In Progress</span>
            <ArrowRight className="w-3 h-3" />
          </div>
        </div>
      </div>

      {/* Zoom controls */}
      <div className="fixed top-20 right-4 z-50 flex flex-col gap-2">
        <Button
          size="icon"
          variant="secondary"
          onClick={handleZoomIn}
          disabled={zoom >= 2}
          className="shadow-lg"
        >
          <ZoomIn className="h-4 w-4" />
        </Button>
        <Button
          size="icon"
          variant="secondary"
          onClick={handleZoomOut}
          disabled={zoom <= 0.5}
          className="shadow-lg"
        >
          <ZoomOut className="h-4 w-4" />
        </Button>
        <Button
          size="icon"
          variant="secondary"
          onClick={handleZoomReset}
          disabled={zoom === 1}
          className="shadow-lg"
        >
          <Maximize2 className="h-4 w-4" />
        </Button>
        <div className="text-xs text-center text-muted-foreground bg-secondary px-2 py-1 rounded">
          {Math.round(zoom * 100)}%
        </div>
      </div>

      {/* Minimap */}
      <div className="fixed bottom-4 right-4 z-50">
        <canvas
          ref={minimapRef}
          className="border border-border/50 rounded bg-background/80 backdrop-blur-sm shadow-lg cursor-pointer"
          width={200}
          height={150}
          onClick={handleMinimapClick}
          onMouseDown={handleMinimapMouseDown}
          onMouseUp={handleMinimapMouseUp}
          onMouseMove={handleMinimapMouseMove}
          onMouseLeave={handleMinimapMouseUp}
        />
      </div>

      <div className="p-8" style={{ width: totalWidth * zoom, height: totalHeight * zoom }}>
        {/* Flow diagram container */}
        <div
          ref={flowContainerRef}
          className="relative rounded-lg border border-border/50 bg-background/50 backdrop-blur-sm overflow-hidden origin-top-left"
          style={{
            width: totalWidth,
            height: totalHeight,
            minHeight: '600px',
            transform: `scale(${zoom})`,
            transformOrigin: '0 0',
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

                // Arrow should point from parent to child (right to left in our layout)
                // Parent is on the right, child is on the left
                const x1 = node.x + CARD_WIDTH; // Parent right edge
                const y1 = node.y + 65; // Middle of parent card
                const x2 = childNode.x; // Child left edge
                const y2 = childNode.y + 65; // Middle of child card

                // Curved path from parent to child
                const midX = (x1 + x2) / 2;
                const path = `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`;

                return (
                  <g key={`${node.task.id}-${childId}`}>
                    <path
                      d={path}
                      fill="none"
                      stroke={childNode.isConvergencePoint ? '#fbbf24' : 'hsl(var(--muted-foreground))'}
                      strokeWidth={childNode.isConvergencePoint ? 3 : 2}
                      strokeDasharray={childNode.isConvergencePoint ? 'none' : '6,4'}
                      opacity={childNode.isConvergencePoint ? 0.9 : 0.7}
                      markerStart={
                        childNode.isConvergencePoint
                          ? 'url(#arrowhead-critical-start)'
                          : 'url(#arrowhead-start)'
                      }
                    />
                  </g>
                );
              })
            )}

            {/* Arrow markers - pointing left (from parent to child) */}
            <defs>
              <marker
                id="arrowhead-start"
                markerWidth="10"
                markerHeight="10"
                refX="1"
                refY="3"
                orient="auto"
              >
                <polygon points="10 0, 0 3, 10 6" fill="hsl(var(--muted-foreground))" opacity="0.7" />
              </marker>
              <marker
                id="arrowhead-critical-start"
                markerWidth="12"
                markerHeight="12"
                refX="2"
                refY="3"
                orient="auto"
              >
                <polygon points="12 0, 0 3, 12 6" fill="#fbbf24" opacity="0.9" />
              </marker>
            </defs>
          </svg>

          {/* Task nodes */}
          {Object.values(nodes).map((node) => (
            <Card
              key={node.task.id}
              className={cn(
                'absolute cursor-pointer',
                'bg-card/95 backdrop-blur-sm border border-border/50',
                'rounded-lg shadow-sm hover:shadow-md',
                'transition-all duration-200',
                'hover:border-primary/50 hover:bg-accent/30 hover:z-20',
                selectedTask?.id === node.task.id &&
                  'ring-2 ring-primary ring-offset-2 ring-offset-background',
                node.isConvergencePoint &&
                  'ring-1 ring-amber-500/30 bg-amber-500/5 border-amber-500/30'
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