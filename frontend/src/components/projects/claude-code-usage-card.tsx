import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2, RefreshCw } from 'lucide-react';

import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import type { ClaudeCodeUsageSnapshot } from 'shared/types';
import { usageApi } from '@/lib/api';

const numberFormatter = new Intl.NumberFormat();

function formatTimeRemaining(seconds: number): string {
  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  if (seconds < 3600) {
    const minutes = Math.round(seconds / 60);
    return `${minutes}m`;
  }
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
}

function UsageProgressBar({
  percent,
  resetLabel,
}: {
  percent: number;
  resetLabel: string;
}) {
  const getColorClass = (pct: number) => {
    if (pct >= 90) return 'bg-red-500';
    if (pct >= 70) return 'bg-yellow-500';
    return 'bg-green-500';
  };

  return (
    <div className="flex items-center gap-2">
      <div className="text-[11px] text-muted-foreground w-24 flex-shrink-0">
        5hr block
      </div>
      <div className="flex-1 h-1 rounded-full bg-muted relative">
        <div
          className={`h-full rounded-full transition-all duration-500 ${getColorClass(percent)}`}
          style={{ width: `${Math.min(100, percent)}%` }}
        />
      </div>
      <div className="text-[10px] text-muted-foreground whitespace-nowrap flex-shrink-0">
        {percent.toFixed(0)}% • {resetLabel}
      </div>
    </div>
  );
}

function UsageSummary({ usage }: { usage: ClaudeCodeUsageSnapshot }) {
  const { t } = useTranslation('projects');

  // Calculate time since block started (5-hour blocks)
  const timeInfo = useMemo(() => {
    const capturedAt = new Date(usage.captured_at);
    const hours = capturedAt.getHours();
    const blockNumber = Math.floor(hours / 5);
    const blockStartHour = blockNumber * 5;

    const blockStart = new Date(capturedAt);
    blockStart.setHours(blockStartHour, 0, 0, 0);

    const now = new Date();
    const blockEnd = new Date(blockStart);
    blockEnd.setHours(blockEnd.getHours() + 5);

    const remaining = Math.max(0, (blockEnd.getTime() - now.getTime()) / 1000);

    return {
      remainingFormatted: formatTimeRemaining(remaining),
    };
  }, [usage.captured_at]);

  const totalTokens = usage.token_usage.total_tokens;

  return (
    <div className="space-y-3">
      <div className="flex items-baseline justify-between">
        <div className="text-2xl font-semibold">{numberFormatter.format(totalTokens)}</div>
        <div className="text-xs text-muted-foreground">
          {t('usage.claudeCode.tokensUsed')}
        </div>
      </div>

      <UsageProgressBar
        percent={usage.used_percent}
        resetLabel={timeInfo.remainingFormatted}
      />
    </div>
  );
}

export function ClaudeCodeUsageCard() {
  const { t } = useTranslation('projects');
  const [usage, setUsage] = useState<ClaudeCodeUsageSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const fetchUsage = async () => {
    setLoading(true);
    setError('');
    try {
      const result = await usageApi.getClaudeCodeUsage();
      setUsage(result);
    } catch (err) {
      console.error('Failed to fetch Claude Code usage', err);
      setError(t('usage.claudeCode.fetchFailed'));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchUsage();

    // Auto-refresh every 30 seconds to pick up usage from running Claude Code sessions
    const interval = setInterval(() => {
      fetchUsage();
    }, 30000);

    return () => clearInterval(interval);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const lastUpdated = useMemo(() => {
    if (!usage) return '';
    const date = new Date(usage.captured_at);
    if (Number.isNaN(date.getTime())) {
      return '';
    }

    return new Intl.DateTimeFormat(undefined, {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: 'numeric',
    }).format(date);
  }, [usage]);

  return (
    <Card className="border border-muted-foreground/20">
      <CardContent className="space-y-4 p-4">
        <div className="flex items-center justify-between gap-2">
          <div>
            <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t('usage.claudeCode.title')}
            </p>
            <h2 className="text-lg font-semibold">Claude Code</h2>
          </div>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={fetchUsage}
            aria-label={t('usage.claudeCode.refresh')}
            disabled={loading}
          >
            {loading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="h-4 w-4" />
            )}
          </Button>
        </div>

        {error && (
          <p className="text-sm text-destructive">
            {error}
          </p>
        )}

        {loading && !usage ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t('usage.claudeCode.loading')}
          </div>
        ) : usage ? (
          <UsageSummary usage={usage} />
        ) : (
          <p className="text-sm text-muted-foreground">
            {t('usage.claudeCode.notAvailable')}
          </p>
        )}

        {usage && lastUpdated && (
          <p className="text-xs text-muted-foreground">
            {t('usage.claudeCode.updated', { time: lastUpdated })}
          </p>
        )}
      </CardContent>
    </Card>
  );
}