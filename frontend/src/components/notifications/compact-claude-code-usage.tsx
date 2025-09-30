import { useEffect, useState } from 'react';
import type { ClaudeCodeUsageSnapshot } from 'shared/types';
import { usageApi } from '@/lib/api';
import {
  CompactProgressBar,
  clampPercent,
  formatCompactDuration,
} from './compact-progress-bar';

function getBlockResetSeconds(capturedAt: string): number | null {
  const timestamp = new Date(capturedAt);
  if (Number.isNaN(timestamp.getTime())) {
    return null;
  }

  const blockStart = new Date(timestamp);
  const blockIndex = Math.floor(blockStart.getHours() / 5);
  blockStart.setHours(blockIndex * 5, 0, 0, 0);

  const blockEnd = new Date(blockStart);
  blockEnd.setHours(blockEnd.getHours() + 5);

  const remainingMs = blockEnd.getTime() - Date.now();
  return Math.max(0, remainingMs / 1000);
}

export function CompactClaudeCodeUsage() {
  const [usage, setUsage] = useState<ClaudeCodeUsageSnapshot | null>(null);

  const fetchUsage = async () => {
    try {
      const result = await usageApi.getClaudeCodeUsage();
      setUsage(result);
    } catch (err) {
      console.error('Failed to fetch Claude Code usage', err);
    }
  };

  useEffect(() => {
    fetchUsage();
    const interval = setInterval(fetchUsage, 30000);
    return () => clearInterval(interval);
  }, []);

  if (!usage) {
    return null;
  }

  const resetSeconds = getBlockResetSeconds(usage.captured_at);
  const resetTime = formatCompactDuration(resetSeconds);

  return (
    <div className="px-3 pb-3 pt-3 border-b border-border shrink-0 w-full min-w-0">
      <div className="space-y-2 w-full min-w-0">
        <div className="text-[10px] font-semibold text-muted-foreground uppercase tracking-wide">
          Claude Code
        </div>
        <CompactProgressBar
          label="5hr block"
          percent={clampPercent(usage.used_percent)}
          resetTime={resetTime || undefined}
        />
      </div>
    </div>
  );
}
