import { useEffect, useMemo, useState } from 'react';
import type { CodexUsageSnapshot } from 'shared/types';
import { usageApi } from '@/lib/api';
import {
  CompactProgressBar,
  clampPercent,
  formatCompactDuration,
} from './compact-progress-bar';

export function CompactCodexUsage() {
  const [usage, setUsage] = useState<CodexUsageSnapshot | null>(null);

  const fetchUsage = async () => {
    try {
      const result = await usageApi.getCodexUsage();
      setUsage(result);
    } catch (err) {
      console.error('Failed to fetch Codex usage', err);
    }
  };

  useEffect(() => {
    fetchUsage();
    const interval = setInterval(fetchUsage, 30000);
    return () => clearInterval(interval);
  }, []);

  const windows = useMemo(() => {
    if (!usage) return [];
    const entries: Array<{
      label: string;
      percent: number;
      resetTime?: string;
    }> = [];

    if (usage.rate_limits.primary) {
      entries.push({
        label: 'Primary',
        percent: clampPercent(usage.rate_limits.primary.used_percent),
        resetTime: formatCompactDuration(
          usage.rate_limits.primary.resets_in_seconds
        ),
      });
    }

    if (usage.rate_limits.secondary) {
      entries.push({
        label: 'Secondary',
        percent: clampPercent(usage.rate_limits.secondary.used_percent),
        resetTime: formatCompactDuration(
          usage.rate_limits.secondary.resets_in_seconds
        ),
      });
    }

    return entries;
  }, [usage]);

  if (!usage || windows.length === 0) {
    return null;
  }

  return (
    <div className="px-3 pb-3 pt-3 border-b border-border shrink-0 w-full min-w-0">
      <div className="space-y-2 w-full min-w-0">
        <div className="text-[10px] font-semibold text-muted-foreground uppercase tracking-wide">
          Codex Limits
        </div>
        {windows.map((window) => (
          <CompactProgressBar
            key={window.label}
            label={window.label}
            percent={window.percent}
            resetTime={window.resetTime}
          />
        ))}
      </div>
    </div>
  );
}
