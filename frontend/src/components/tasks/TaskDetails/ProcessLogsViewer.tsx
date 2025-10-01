import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso';
import { AlertCircle } from 'lucide-react';
import { useLogStream } from '@/hooks/useLogStream';
import RawLogText from '@/components/common/RawLogText';
import type { PatchType } from 'shared/types';

type LogEntry = Extract<PatchType, { type: 'STDOUT' } | { type: 'STDERR' }>;

// Add unique ID to log entries for stable React keys
type LogEntryWithId = LogEntry & { _id: string };

interface ProcessLogsViewerProps {
  processId: string;
}

export default function ProcessLogsViewer({
  processId,
}: ProcessLogsViewerProps) {
  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const didInitScroll = useRef(false);
  const prevLenRef = useRef(0);
  const [atBottom, setAtBottom] = useState(true);
  const logIdCounterRef = useRef(0);

  const { logs, error } = useLogStream(processId);

  // Add stable IDs to logs for React keys (prevents index-based issues)
  const logsWithIds = useMemo<LogEntryWithId[]>(() => {
    const currentCount = logIdCounterRef.current;
    const newLogs = logs.slice(currentCount).map((log, idx) => ({
      ...log,
      _id: `${processId}:${currentCount + idx}`,
    }));

    logIdCounterRef.current = logs.length;

    // Merge with existing logs
    if (currentCount === 0) {
      return newLogs;
    }
    return [...logs.slice(0, currentCount).map((log, idx) => ({
      ...log,
      _id: `${processId}:${idx}`,
    } as LogEntryWithId)), ...newLogs];
  }, [logs, processId]);

  // Reset counter when process changes
  useEffect(() => {
    logIdCounterRef.current = 0;
    didInitScroll.current = false;
  }, [processId]);

  // 1) Initial jump to bottom once data appears.
  useEffect(() => {
    if (!didInitScroll.current && logsWithIds.length > 0) {
      didInitScroll.current = true;
      requestAnimationFrame(() => {
        virtuosoRef.current?.scrollToIndex({
          index: logsWithIds.length - 1,
          align: 'end',
        });
      });
    }
  }, [logsWithIds.length]);

  // 2) If there's a large append and we're at bottom, force-stick to the last item.
  useEffect(() => {
    const prev = prevLenRef.current;
    const grewBy = logsWithIds.length - prev;
    prevLenRef.current = logsWithIds.length;

    // tweak threshold as you like; this handles "big bursts"
    const LARGE_BURST = 10;
    if (grewBy >= LARGE_BURST && atBottom && logsWithIds.length > 0) {
      // defer so Virtuoso can re-measure before jumping
      requestAnimationFrame(() => {
        virtuosoRef.current?.scrollToIndex({
          index: logsWithIds.length - 1,
          align: 'end',
        });
      });
    }
  }, [logsWithIds.length, atBottom]);

  const formatLogLine = useCallback((_index: number, entry: LogEntryWithId) => {
    return (
      <RawLogText
        key={entry._id}
        content={entry.content}
        channel={entry.type === 'STDERR' ? 'stderr' : 'stdout'}
        className="text-sm px-4 py-1"
      />
    );
  }, []);

  return (
    <div className="h-full">
      {logsWithIds.length === 0 && !error ? (
        <div className="p-4 text-center text-muted-foreground text-sm">
          No logs available
        </div>
      ) : error ? (
        <div className="p-4 text-center text-destructive text-sm">
          <AlertCircle className="h-4 w-4 inline mr-2" />
          {error}
        </div>
      ) : (
        <Virtuoso<LogEntryWithId>
          ref={virtuosoRef}
          className="flex-1 rounded-lg"
          data={logsWithIds}
          itemContent={formatLogLine}
          computeItemKey={(_index, entry) => entry._id}
          // Keep pinned while user is at bottom; release when they scroll up
          atBottomStateChange={setAtBottom}
          followOutput={atBottom ? 'smooth' : false}
          // Optional: a bit more overscan helps during bursts
          increaseViewportBy={{ top: 0, bottom: 600 }}
        />
      )}
    </div>
  );
}
