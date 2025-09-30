import { useEffect, useMemo, useState } from 'react';
import type { CodexUsageSnapshot } from 'shared/types';
import { usageApi } from '@/lib/api';

function clampPercent(value: number | undefined | null) {
  if (value === undefined || value === null || Number.isNaN(value)) {
    return 0;
  }
  return Math.min(100, Math.max(0, value));
}

interface CompactProgressBarProps {
  label: string;
  percent: number;
  resetTime?: string;
}

function formatResetTime(seconds: number | null | undefined): string {
  if (seconds === undefined || seconds === null) return '';

  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  if (seconds < 3600) {
    return `${Math.round(seconds / 60)}m`;
  }
  if (seconds < 86400) {
    return `${Math.round(seconds / 3600)}h`;
  }
  return `${Math.round(seconds / 86400)}d`;
}

function CompactProgressBar({ label, percent, resetTime }: CompactProgressBarProps) {
  const getColorClass = (pct: number) => {
    if (pct >= 90) return 'bg-red-500';
    if (pct >= 70) return 'bg-yellow-500';
    return 'bg-green-500';
  };

  return (
    <div className="space-y-1 w-full min-w-0">
      <div className="flex items-center justify-between text-[10px] gap-2">
        <span className="text-muted-foreground truncate">{label}</span>
        <div className="flex items-center gap-1.5 whitespace-nowrap">
          <span className="font-medium">{percent.toFixed(0)}%</span>
          {resetTime && (
            <>
              <span className="text-muted-foreground">•</span>
              <span className="text-muted-foreground">{resetTime}</span>
            </>
          )}
        </div>
      </div>
      <div className="h-1 w-full rounded-full bg-muted relative overflow-hidden">
        <div
          className={`h-full rounded-full transition-all duration-500 ${getColorClass(percent)}`}
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

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
        resetTime: formatResetTime(usage.rate_limits.primary.resets_in_seconds),
      });
    }

    if (usage.rate_limits.secondary) {
      entries.push({
        label: 'Secondary',
        percent: clampPercent(usage.rate_limits.secondary.used_percent),
        resetTime: formatResetTime(usage.rate_limits.secondary.resets_in_seconds),
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