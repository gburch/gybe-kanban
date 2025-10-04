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
  ArrowRight,
  ZoomIn,
  ZoomOut,
  Maximize2,
} from 'lucide-react';
import dagre from 'dagre';

const CARD_WIDTH = 280;
const CARD_HEIGHT = 110;
const FLOW_NODE_VERTICAL_GAP = 6;
const FLOW_RANK_HORIZONTAL_GAP = 200;
const FLOW_STATUS_ZONE_GAP = 24;
const FLOW_LAYOUT_MARGIN_X = 24;
const FLOW_LAYOUT_MARGIN_Y = 24;
const FLOW_BASE_X = 24;

type Task = TaskWithAttemptStatus;

interface ParentTaskSummary {
  id: string;
  title: string;
}

interface TaskFlowViewProps {
  tasks: Task[];
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

interface ZoneBounds {
  start: number;
  width: number;
}

interface LayoutZones {
  done: ZoneBounds;
  todo: ZoneBounds;
  active: ZoneBounds;
}

interface FlowLayout {
  nodes: Record<string, FlowNode>;
  totalWidth: number;
  totalHeight: number;
  focusX: number;
  zones: LayoutZones;
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
): FlowLayout {
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

  // Mark critical path - tasks that are convergence points or lead to them
  const criticalPathNodes = new Set<string>();
  const markCriticalPath = (nodeId: string) => {
    if (criticalPathNodes.has(nodeId)) return;
    criticalPathNodes.add(nodeId);
    const node = nodes[nodeId];
    if (node) {
      node.parents.forEach(parentId => markCriticalPath(parentId));
    }
  };

  // Start from convergence points and mark their ancestor paths
  Object.values(nodes).forEach(node => {
    if (node.isConvergencePoint) {
      markCriticalPath(node.task.id);
    }
  });

  // Create Dagre graph for hierarchical layout
  const g = new dagre.graphlib.Graph();
  g.setGraph({
    rankdir: 'LR', // Left to right - we'll reverse edge direction to get children left of parents
    nodesep: FLOW_NODE_VERTICAL_GAP,
    ranksep: FLOW_RANK_HORIZONTAL_GAP,
    marginx: FLOW_LAYOUT_MARGIN_X,
    marginy: FLOW_LAYOUT_MARGIN_Y,
    ranker: 'tight-tree', // Use tight-tree ranker to respect rank constraints
  });
  g.setDefaultEdgeLabel(() => ({}));

  // Add nodes to Dagre with rank constraints based on status
  // Rank determines horizontal position in LR layout: lower rank = further left
  // Layout: DONE (left/past) → IN PROGRESS (middle) → TODO (right/future)
  Object.values(nodes).forEach((node) => {
    const status = node.task.status.toLowerCase();
    let rank: number | undefined;

    // Set rank based on status to align tasks horizontally by status
    if (status === 'done' || status === 'cancelled') {
      rank = 0; // Leftmost - completed tasks (past)
    } else if (status === 'inprogress' || status === 'inreview') {
      rank = 1; // Middle - active work (present)
    } else if (status === 'todo') {
      rank = 2; // Rightmost - future tasks
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

  // Adjust X positions to respect both hierarchy AND status
  // Use Dagre's X positions but shift them based on status zones
  // This preserves parent-child relationships while grouping by status
  // Layout: DONE (left/past) → TODO (middle) → IN PROGRESS/PARENT (right/active work)
  // Children come before parents, so completed children and pending todos are on the left

  // Find the min/max X for each status group from Dagre layout
  const statusGroups: Record<string, { nodes: FlowNode[]; minX: number; maxX: number }> = {
    done: { nodes: [], minX: Infinity, maxX: -Infinity },
    active: { nodes: [], minX: Infinity, maxX: -Infinity },
    todo: { nodes: [], minX: Infinity, maxX: -Infinity },
  };

  Object.values(nodes).forEach(node => {
    const status = node.task.status.toLowerCase();
    let group: 'done' | 'active' | 'todo';

    if (status === 'done' || status === 'cancelled') {
      group = 'done';
    } else if (status === 'inprogress' || status === 'inreview') {
      group = 'active';
    } else {
      group = 'todo';
    }

    statusGroups[group].nodes.push(node);
    statusGroups[group].minX = Math.min(statusGroups[group].minX, node.x);
    statusGroups[group].maxX = Math.max(statusGroups[group].maxX, node.x + CARD_WIDTH);
  });

  // Calculate the width of each status group
  const getZoneWidth = (group: { nodes: FlowNode[]; minX: number; maxX: number }) => {
    if (group.nodes.length === 0) {
      return CARD_WIDTH;
    }
    return Math.max(group.maxX - group.minX, CARD_WIDTH);
  };

  const doneWidth = getZoneWidth(statusGroups.done);
  const todoWidth = getZoneWidth(statusGroups.todo);
  const activeWidth = getZoneWidth(statusGroups.active);

  // Define horizontal zones with gaps between status groups
  // Order: Done (left) → Todo (middle) → Active (right)
  const baseX = FLOW_BASE_X;
  const statusGap = FLOW_STATUS_ZONE_GAP;

  const doneZoneStart = baseX;
  const todoZoneStart = doneZoneStart + doneWidth + statusGap;
  const activeZoneStart = todoZoneStart + todoWidth + statusGap;

  // Shift each status group to its zone while preserving relative positions
  statusGroups.done.nodes.forEach(node => {
    node.x = doneZoneStart + (node.x - statusGroups.done.minX);
  });

  statusGroups.todo.nodes.forEach(node => {
    node.x = todoZoneStart + (node.x - statusGroups.todo.minX);
  });

  statusGroups.active.nodes.forEach(node => {
    node.x = activeZoneStart + (node.x - statusGroups.active.minX);
  });

  const computeZoneBounds = (
    group: { nodes: FlowNode[] },
    fallbackStart: number,
    fallbackWidth: number
  ): ZoneBounds => {
    if (group.nodes.length === 0) {
      return {
        start: fallbackStart,
        width: fallbackWidth,
      };
    }

    const min = Math.min(...group.nodes.map((n) => n.x));
    const max = Math.max(...group.nodes.map((n) => n.x + CARD_WIDTH));
    return {
      start: min,
      width: Math.max(max - min, fallbackWidth),
    };
  };

  const zones: LayoutZones = {
    done: computeZoneBounds(statusGroups.done, doneZoneStart, doneWidth),
    todo: computeZoneBounds(statusGroups.todo, todoZoneStart, todoWidth),
    active: computeZoneBounds(statusGroups.active, activeZoneStart, activeWidth),
  };

  // Calculate bounds
  const allNodes = Object.values(nodes);
  const maxX = Math.max(...allNodes.map((n) => n.x + CARD_WIDTH), activeZoneStart + activeWidth + 120);
  const maxY = Math.max(...allNodes.map((n) => n.y + CARD_HEIGHT), 800);

  // Find active tasks for initial scroll position
  // Active work is on the right (parents), scroll to show them
  const activeNodesForFocus = allNodes.filter(n =>
    n.task.status.toLowerCase() === 'inprogress' ||
    n.task.status.toLowerCase() === 'inreview'
  );
  const focusX = activeNodesForFocus.length > 0
    ? Math.min(...activeNodesForFocus.map(n => n.x)) // Start of active zone
    : 0;

  return {
    nodes,
    totalWidth: maxX + 80,
    totalHeight: maxY + 80,
    focusX, // X position to scroll to
    zones,
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
  const { nodes, totalWidth, totalHeight, focusX, zones } = useMemo(
    () => buildFlowLayout(tasks, parentTasksById),
    [tasks, parentTasksById]
  );

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const flowContainerRef = useRef<HTMLDivElement>(null);
  const minimapRef = useRef<HTMLCanvasElement>(null);
  const [zoom, setZoom] = useState(1);
  const [isMinimapDragging, setIsMinimapDragging] = useState(false);

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

  // Scroll the selected task into view when changed
  useEffect(() => {
    if (!selectedTask || !scrollContainerRef.current) return;
    const node = nodes[selectedTask.id];
    if (!node) return;

    const container = scrollContainerRef.current;
    const targetX = node.x * zoom + (CARD_WIDTH * zoom) / 2 - container.clientWidth / 2;
    const targetY = node.y * zoom + (CARD_HEIGHT * zoom) / 2 - container.clientHeight / 2;

    container.scrollTo({
      left: Math.max(0, targetX),
      top: Math.max(0, targetY),
      behavior: 'smooth',
    });
  }, [selectedTask?.id, nodes, zoom]);

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
      Object.values(nodes).forEach((node) => {
        const x = node.x * scale;
        const y = node.y * scale;
        const w = CARD_WIDTH * scale;
        const h = CARD_HEIGHT * scale;

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

  const { highlightedNodes, highlightedEdges } = useMemo(() => {
    if (!selectedTask?.id) {
      return {
        highlightedNodes: new Set<string>(),
        highlightedEdges: new Set<string>(),
      };
    }

    const resultNodes = new Set<string>();
    const resultEdges = new Set<string>();
    const queue: string[] = [selectedTask.id];
    let index = 0;

    while (index < queue.length) {
      const currentId = queue[index++];
      if (resultNodes.has(currentId)) continue;
      resultNodes.add(currentId);

      const current = nodes[currentId];
      if (!current) continue;

      current.parents.forEach((parentId) => {
        resultEdges.add(`${currentId}->${parentId}`);
        if (!resultNodes.has(parentId)) {
          queue.push(parentId);
        }
      });

      current.children.forEach((childId) => {
        resultEdges.add(`${childId}->${currentId}`);
        if (!resultNodes.has(childId)) {
          queue.push(childId);
        }
      });
    }

    return {
      highlightedNodes: resultNodes,
      highlightedEdges: resultEdges,
    };
  }, [nodes, selectedTask?.id]);

  const hasHighlights = highlightedNodes.size > 0;

  const statusBandMeta: Record<keyof LayoutZones, { label: string; className: string; textClass: string }> = {
    done: {
      label: 'Completed',
      className: 'bg-emerald-500/5 border-r border-emerald-500/10',
      textClass: 'text-emerald-300/80',
    },
    todo: {
      label: 'Queued',
      className: 'bg-amber-500/5 border-r border-amber-500/10',
      textClass: 'text-amber-300/80',
    },
    active: {
      label: 'Active',
      className: 'bg-sky-500/5',
      textClass: 'text-sky-300/80',
    },
  };

  return (
    <div ref={scrollContainerRef} className="w-full h-full bg-background overflow-auto relative">
      {/* Legend - Sticky at top */}
      <div className="sticky top-0 z-40 bg-background/95 backdrop-blur-sm border-b border-border/50 px-8 py-3">
        <div className="flex flex-wrap items-center gap-6 text-sm text-muted-foreground">
          <div className="flex items-center gap-2">
            <svg width="32" height="2" className="opacity-70">
              <line x1="0" y1="1" x2="32" y2="1" stroke="#71717a" strokeWidth="2" strokeDasharray="6,4" />
            </svg>
            <span>Dependency</span>
          </div>
          <div className="flex items-center gap-2">
            <svg width="32" height="3">
              <line x1="0" y1="1.5" x2="32" y2="1.5" stroke="#fbbf24" strokeWidth="3" />
            </svg>
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
          <div className="flex items-center gap-3 ml-auto">
            <span className="text-xs text-muted-foreground mr-2">Flow:</span>
            <div className="flex items-center gap-2 text-xs">
              <div className="w-3 h-3 rounded-sm bg-green-500/40" />
              <span>Done</span>
            </div>
            <ArrowRight className="h-3 w-3 text-muted-foreground" />
            <div className="flex items-center gap-2 text-xs">
              <div className="w-3 h-3 rounded-sm bg-zinc-400/40" />
              <span>Todo</span>
            </div>
            <ArrowRight className="h-3 w-3 text-muted-foreground" />
            <div className="flex items-center gap-2 text-xs">
              <div className="w-3 h-3 rounded-sm bg-blue-500/60" />
              <span>Active</span>
            </div>
          </div>
        </div>
      </div>

      {/* Zoom controls */}
      <div className="fixed top-20 right-4 z-30 flex flex-col gap-2">
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
      <div className="fixed bottom-4 right-4 z-30">
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

      <div className="p-4" style={{ width: totalWidth * zoom, height: totalHeight * zoom }}>
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

          {/* Status zones */}
          <div className="absolute inset-0 pointer-events-none" style={{ zIndex: 0 }}>
            {(Object.keys(zones) as Array<keyof LayoutZones>).map((zoneKey) => {
              const zone = zones[zoneKey];
              const meta = statusBandMeta[zoneKey];
              return (
                <div
                  key={zoneKey}
                  className={cn(
                    'absolute top-0 bottom-0 px-4 py-3 flex items-start',
                    meta.className
                  )}
                  style={{
                    left: `${zone.start}px`,
                    width: `${Math.max(zone.width, CARD_WIDTH)}px`,
                  }}
                >
                  <span
                    className={cn(
                      'text-xs font-medium uppercase tracking-wider text-muted-foreground',
                      meta.textClass
                    )}
                  >
                    {meta.label}
                  </span>
                </div>
              );
            })}
          </div>

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

                // Determine arrow direction based on relative positions
                // If child is left of parent: arrow goes child-right → parent-left
                // If child is right of parent: arrow goes child-left → parent-right
                const childIsLeft = childNode.x < node.x;

                let x1: number, y1: number, x2: number, y2: number;

                if (childIsLeft) {
                  // Child is on the left, parent on the right (normal flow)
                  x1 = childNode.x + CARD_WIDTH; // Child's right edge
                  y1 = childNode.y + CARD_HEIGHT / 2;
                  x2 = node.x; // Parent's left edge
                  y2 = node.y + CARD_HEIGHT / 2;
                } else {
                  // Child is on the right, parent on the left (reversed due to status)
                  x1 = childNode.x; // Child's left edge
                  y1 = childNode.y + CARD_HEIGHT / 2;
                  x2 = node.x + CARD_WIDTH; // Parent's right edge
                  y2 = node.y + CARD_HEIGHT / 2;
                }

                // Curved path
                const midX = (x1 + x2) / 2;
                const path = `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`;

                // Check if this connection is on the critical path
                const isCriticalConnection = node.isConvergencePoint || childNode.isConvergencePoint;
                const edgeKey = `${childId}->${node.task.id}`;
                const isHighlightedEdge = highlightedEdges.has(edgeKey);

                let stroke = '#71717a';
                let strokeWidth = 2;
                let dashArray: string | undefined = '6,4';
                let opacity = hasHighlights ? 0.2 : 0.7;

                if (isCriticalConnection) {
                  stroke = '#fbbf24';
                  strokeWidth = 3;
                  dashArray = undefined;
                  opacity = hasHighlights ? 0.3 : 0.9;
                }

                if (isHighlightedEdge) {
                  stroke = '#38bdf8';
                  strokeWidth = isCriticalConnection ? 4 : 3;
                  dashArray = undefined;
                  opacity = 0.95;
                }

                return (
                  <g key={`${node.task.id}-${childId}`}>
                    <path
                      d={path}
                      fill="none"
                      stroke={stroke}
                      strokeWidth={strokeWidth}
                      strokeDasharray={dashArray}
                      opacity={opacity}
                      markerEnd={
                        isHighlightedEdge
                          ? 'url(#arrowhead-highlight)'
                          : isCriticalConnection
                            ? 'url(#arrowhead-critical)'
                            : 'url(#arrowhead)'
                      }
                    />
                  </g>
                );
              })
            )}

            {/* Arrow markers - pointing right (from child to parent) */}
            <defs>
              <marker
                id="arrowhead"
                markerWidth="10"
                markerHeight="10"
                refX="9"
                refY="3"
                orient="auto"
              >
                <polygon points="0 0, 10 3, 0 6" fill="#71717a" opacity="0.7" />
              </marker>
              <marker
                id="arrowhead-critical"
                markerWidth="12"
                markerHeight="12"
                refX="10"
                refY="3"
                orient="auto"
              >
                <polygon points="0 0, 12 3, 0 6" fill="#fbbf24" opacity="0.9" />
              </marker>
              <marker
                id="arrowhead-highlight"
                markerWidth="12"
                markerHeight="12"
                refX="10"
                refY="3"
                orient="auto"
              >
                <polygon points="0 0, 12 3, 0 6" fill="#38bdf8" opacity="0.95" />
              </marker>
            </defs>
          </svg>

          {/* Task nodes */}
          {Object.values(nodes).map((node) => {
            const status = node.task.status.toLowerCase();
            const isDone = status === 'done' || status === 'cancelled';
            const isActive = status === 'inprogress' || status === 'inreview';
            const isTodo = status === 'todo';
            const isSelected = selectedTask?.id === node.task.id;
            const isHighlighted = highlightedNodes.has(node.task.id);
            const isDimmed = hasHighlights && !isHighlighted && !isSelected;

            return (
            <Card
              key={node.task.id}
              className={cn(
                'absolute cursor-pointer',
                'backdrop-blur-sm',
                'rounded-lg shadow-sm hover:shadow-md',
                'transition-all duration-200',
                'hover:z-[60]',
                // Status-based colors matching legend
                isDone && 'bg-green-500/10 border-green-500/30 hover:bg-green-500/15',
                isActive && 'bg-blue-500/15 border-blue-500/40 hover:bg-blue-500/20',
                isTodo && 'bg-zinc-700/20 border-zinc-600/40 hover:bg-zinc-700/30',
                // Selected state
                isSelected && 'ring-2 ring-primary ring-offset-2 ring-offset-background z-[60]',
                // Critical path highlighting (stronger border + glow)
                node.isConvergencePoint &&
                  'ring-1 ring-amber-400/60 border-amber-400/70 shadow-amber-400/20',
                isHighlighted && !isSelected && 'border-sky-500/40 ring-1 ring-sky-400/30',
                isDimmed && 'opacity-45 hover:opacity-80'
              )}
              style={{
                left: `${node.x}px`,
                top: `${node.y}px`,
                width: `${CARD_WIDTH}px`,
                zIndex: isSelected ? 60 : 10,
              }}
              onClick={() => onViewTaskDetails(node.task)}
            >
              <CardContent className="p-3">
                {/* Convergence indicator */}
                {node.isConvergencePoint && (
                  <div className="flex items-center gap-1 mb-1 text-xs font-semibold text-amber-400">
                    <GitMerge className="h-3 w-3" />
                    <span>Critical Path</span>
                  </div>
                )}

                {/* Branch indicator */}
                {node.isBranchPoint && (
                  <div className="flex items-center gap-1 mb-1 text-xs font-semibold text-blue-400">
                    <GitBranch className="h-3 w-3" />
                    <span>Branches</span>
                  </div>
                )}

                {/* Task content */}
                <div className="flex items-start gap-2 mb-1.5">
                  {getStatusIcon(node.task)}
                  <div className="flex-1 min-w-0">
                    <h3 className="font-medium text-sm line-clamp-2 text-foreground">
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
                      {node.children.length} child{node.children.length > 1 ? 'ren' : ''}
                    </span>
                  )}
                </div>
              </CardContent>
            </Card>
            );
          })}
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
