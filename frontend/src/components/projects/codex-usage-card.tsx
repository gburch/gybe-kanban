import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2, RefreshCw } from 'lucide-react';

import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import type { CodexUsageSnapshot, CodexUsageWindow } from 'shared/types';
import { usageApi } from '@/lib/api';

const numberFormatter = new Intl.NumberFormat();

function clampPercent(value: number | undefined | null) {
  if (value === undefined || value === null || Number.isNaN(value)) {
    return 0;
  }
  return Math.min(100, Math.max(0, value));
}

function formatResetLabel(
  seconds: number | null | undefined,
  fallback: string,
  format: (input: string) => string
) {
  if (seconds === undefined || seconds === null) {
    return fallback;
  }

  if (seconds < 60) {
    const value = Math.round(seconds);
    return format(`${value}s`);
  }

  if (seconds < 3600) {
    const minutes = Math.round(seconds / 60);
    return format(`${minutes}m`);
  }

  if (seconds < 86400) {
    const hours = Math.round(seconds / 3600);
    return format(`${hours}h`);
  }

  const days = Math.round(seconds / 86400);
  return format(`${days}d`);
}

function StackedProgressBar({
  label,
  percent,
  resetLabel,
}: {
  label: string;
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
      <div className="text-[11px] text-muted-foreground w-20 flex-shrink-0">
        {label}
      </div>
      <div className="flex-1 h-1 rounded-full bg-muted relative">
        <div
          className={`h-full rounded-full transition-all duration-500 ${getColorClass(percent)}`}
          style={{ width: `${percent}%` }}
        />
      </div>
      <div className="text-[10px] text-muted-foreground whitespace-nowrap flex-shrink-0">
        {percent.toFixed(0)}% • {resetLabel}
      </div>
    </div>
  );
}

function UsageSummary({ usage }: { usage: CodexUsageSnapshot }) {
  const { t } = useTranslation('projects');

  const windows = useMemo(() => {
    const entries: Array<{
      key: 'primary' | 'secondary';
      label: string;
      window: CodexUsageWindow;
    }> = [];

    if (usage.rate_limits.primary) {
      entries.push({
        key: 'primary',
        label: t('usage.codex.primaryLimit'),
        window: usage.rate_limits.primary,
      });
    }

    if (usage.rate_limits.secondary) {
      entries.push({
        key: 'secondary',
        label: t('usage.codex.secondaryLimit'),
        window: usage.rate_limits.secondary,
      });
    }

    return entries;
  }, [t, usage.rate_limits.primary, usage.rate_limits.secondary]);

  const formatReset = (seconds: number | null | undefined) =>
    formatResetLabel(
      seconds,
      t('usage.codex.resetUnknown'),
      (value) => t('usage.codex.resetIn', { time: value })
    );

  return (
    <div className="space-y-1.5">
      {windows.length > 0 ? (
        windows.map(({ key, label, window }) => (
          <StackedProgressBar
            key={key}
            label={label}
            percent={clampPercent(window.used_percent)}
            resetLabel={formatReset(window.resets_in_seconds)}
          />
        ))
      ) : (
        <p className="text-sm text-muted-foreground">
          {t('usage.codex.notAvailable')}
        </p>
      )}
    </div>
  );
}

export function CodexUsageCard() {
  const { t } = useTranslation('projects');
  const [usage, setUsage] = useState<CodexUsageSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const fetchUsage = async () => {
    setLoading(true);
    setError('');
    try {
      const result = await usageApi.getCodexUsage();
      setUsage(result);
    } catch (err) {
      console.error('Failed to fetch Codex usage', err);
      setError(t('usage.codex.fetchFailed'));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchUsage();
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
              {t('usage.codex.title')}
            </p>
            <h2 className="text-lg font-semibold">Codex</h2>
          </div>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={fetchUsage}
            aria-label={t('usage.codex.refresh')}
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
            {t('usage.codex.loading')}
          </div>
        ) : usage ? (
          <UsageSummary usage={usage} />
        ) : (
          <p className="text-sm text-muted-foreground">
            {t('usage.codex.notAvailable')}
          </p>
        )}

        {usage && lastUpdated && (
          <p className="text-xs text-muted-foreground">
            {t('usage.codex.updated', { time: lastUpdated })}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
