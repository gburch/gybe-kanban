interface CompactProgressBarProps {
  label: string;
  percent: number;
  resetTime?: string;
}

export function clampPercent(value: number | undefined | null): number {
  if (value === undefined || value === null || Number.isNaN(value)) {
    return 0;
  }
  return Math.min(100, Math.max(0, value));
}

export function formatCompactDuration(seconds: number | null | undefined): string {
  if (seconds === undefined || seconds === null || Number.isNaN(seconds)) {
    return '';
  }

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

export function CompactProgressBar({ label, percent, resetTime }: CompactProgressBarProps) {
  const normalizedPercent = clampPercent(percent);

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
          <span className="font-medium">{normalizedPercent.toFixed(0)}%</span>
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
          className={`h-full rounded-full transition-all duration-500 ${getColorClass(normalizedPercent)}`}
          style={{ width: `${normalizedPercent}%` }}
        />
      </div>
    </div>
  );
}
