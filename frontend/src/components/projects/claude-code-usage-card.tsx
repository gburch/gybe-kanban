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

function UsageSummary({ usage }: { usage: ClaudeCodeUsageSnapshot }) {
  const { t } = useTranslation('projects');
  const { token_usage } = usage;

  // Calculate time since block started (5-hour blocks)
  const blockStart = useMemo(() => {
    const capturedAt = new Date(usage.captured_at);
    const hours = capturedAt.getHours();
    // Find the start of the current 5-hour block
    const blockNumber = Math.floor(hours / 5);
    const blockStartHour = blockNumber * 5;

    const blockStartTime = new Date(capturedAt);
    blockStartTime.setHours(blockStartHour, 0, 0, 0);

    return blockStartTime;
  }, [usage.captured_at]);

  const timeInfo = useMemo(() => {
    const now = new Date();
    const blockEnd = new Date(blockStart);
    blockEnd.setHours(blockEnd.getHours() + 5);

    const elapsed = (now.getTime() - blockStart.getTime()) / 1000;
    const remaining = Math.max(0, (blockEnd.getTime() - now.getTime()) / 1000);

    return {
      elapsed,
      remaining,
      elapsedFormatted: formatTimeRemaining(elapsed),
      remainingFormatted: formatTimeRemaining(remaining),
    };
  }, [blockStart]);

  const totalTokens = token_usage.total_tokens;

  return (
    <div className="space-y-3">
      <div className="flex items-baseline justify-between">
        <div className="text-2xl font-semibold">{numberFormatter.format(totalTokens)}</div>
        <div className="text-xs text-muted-foreground">
          {t('usage.claudeCode.tokensUsed')}
        </div>
      </div>

      <div className="text-xs text-muted-foreground">
        {t('usage.claudeCode.currentBlock', {
          elapsed: timeInfo.elapsedFormatted,
          remaining: timeInfo.remainingFormatted
        })}
      </div>

      {usage.session_info.git_branch && (
        <div className="text-xs text-muted-foreground pt-1 border-t">
          {t('usage.claudeCode.branch')}: {usage.session_info.git_branch}
        </div>
      )}
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