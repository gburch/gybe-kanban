// streamJsonPatchEntries.ts - WebSocket JSON patch streaming utility
import { applyPatch, type Operation } from 'rfc6902';

type PatchContainer<E = unknown> = { entries: E[] };

export interface StreamOptions<E = unknown> {
  initial?: PatchContainer<E>;
  /** called after each successful patch application */
  onEntries?: (entries: E[]) => void;
  onConnect?: () => void;
  onError?: (err: unknown) => void;
  /** called once when a "finished" event is received */
  onFinished?: (entries: E[]) => void;
}

interface StreamController<E = unknown> {
  /** Current entries array (immutable snapshot) */
  getEntries(): E[];
  /** Full { entries } snapshot */
  getSnapshot(): PatchContainer<E>;
  /** Best-effort connection state */
  isConnected(): boolean;
  /** Subscribe to updates; returns an unsubscribe function */
  onChange(cb: (entries: E[]) => void): () => void;
  /** Close the stream */
  close(): void;
}

/**
 * Connect to a WebSocket endpoint that emits JSON messages containing:
 *   {"JsonPatch": [{"op": "add", "path": "/entries/0", "value": {...}}, ...]}
 *   {"Finished": ""}
 *
 * Maintains an in-memory { entries: [] } snapshot and returns a controller.
 */
export function streamJsonPatchEntries<E = unknown>(
  url: string,
  opts: StreamOptions<E> = {}
): StreamController<E> {
  let connected = false;
  let snapshot: PatchContainer<E> = structuredClone(
    opts.initial ?? ({ entries: [] } as PatchContainer<E>)
  );

  const subscribers = new Set<(entries: E[]) => void>();
  if (opts.onEntries) subscribers.add(opts.onEntries);

  // Convert HTTP endpoint to WebSocket endpoint
  const wsUrl = url.replace(/^http/, 'ws');
  const ws = new WebSocket(wsUrl);

  const notify = () => {
    for (const cb of subscribers) {
      try {
        cb(snapshot.entries);
      } catch {
        /* swallow subscriber errors */
      }
    }
  };

  // Batch patches to reduce structuredClone overhead
  let pendingOps: Operation[] = [];
  let batchTimer: ReturnType<typeof setTimeout> | null = null;
  const BATCH_DELAY = 50; // ms

  const flushPatches = () => {
    if (pendingOps.length === 0) return;

    const ops = dedupeOps(pendingOps);
    pendingOps = [];
    batchTimer = null;

    // Apply to a working copy (applyPatch mutates)
    const next = structuredClone(snapshot);
    applyPatch(next as unknown as object, ops);

    snapshot = next;
    notify();
  };

  const handleMessage = (event: MessageEvent) => {
    try {
      const msg = JSON.parse(event.data);

      // Handle JsonPatch messages (from LogMsg::to_ws_message)
      if (msg.JsonPatch) {
        const raw = msg.JsonPatch as Operation[];
        pendingOps.push(...raw);

        // Batch multiple rapid patches together
        if (batchTimer !== null) {
          clearTimeout(batchTimer);
        }
        batchTimer = setTimeout(flushPatches, BATCH_DELAY);
      }

      // Handle Finished messages
      if (msg.finished !== undefined) {
        // Flush any pending patches before finishing
        if (batchTimer !== null) {
          clearTimeout(batchTimer);
          flushPatches();
        }
        opts.onFinished?.(snapshot.entries);
        ws.close();
      }
    } catch (err) {
      opts.onError?.(err);
    }
  };

  ws.addEventListener('open', () => {
    connected = true;
    opts.onConnect?.();
  });

  ws.addEventListener('message', handleMessage);

  ws.addEventListener('error', (err) => {
    connected = false;
    opts.onError?.(err);
  });

  ws.addEventListener('close', () => {
    connected = false;
  });

  return {
    getEntries(): E[] {
      return snapshot.entries;
    },
    getSnapshot(): PatchContainer<E> {
      return snapshot;
    },
    isConnected(): boolean {
      return connected;
    },
    onChange(cb: (entries: E[]) => void): () => void {
      subscribers.add(cb);
      // push current state immediately
      cb(snapshot.entries);
      return () => subscribers.delete(cb);
    },
    close(): void {
      if (batchTimer !== null) {
        clearTimeout(batchTimer);
        batchTimer = null;
      }
      ws.close();
      subscribers.clear();
      connected = false;
    },
  };
}

/**
 * Dedupe multiple ops that touch the same path within a single event.
 * Last write for a path wins, while preserving the overall left-to-right
 * order of the *kept* final operations.
 *
 * Example:
 *   add /entries/4, replace /entries/4  -> keep only the final replace
 */
function dedupeOps(ops: Operation[]): Operation[] {
  const lastIndexByPath = new Map<string, number>();
  ops.forEach((op, i) => lastIndexByPath.set(op.path, i));

  // Keep only the last op for each path, in ascending order of their final index
  const keptIndices = [...lastIndexByPath.values()].sort((a, b) => a - b);
  return keptIndices.map((i) => ops[i]!);
}
