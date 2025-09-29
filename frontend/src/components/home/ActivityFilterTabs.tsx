import { ActivityFeedFilter } from '@/lib/api';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface ActivityFilterTabsProps {
  active: ActivityFeedFilter;
  onSelect: (filter: ActivityFeedFilter) => void;
  disabled?: boolean;
}

const FILTERS: Array<{ label: string; value: ActivityFeedFilter; shortLabel?: string }> = [
  { label: 'Needs Review', value: 'need_review', shortLabel: 'Review' },
  { label: 'In Progress', value: 'in_progress', shortLabel: 'Progress' },
  { label: 'Completed', value: 'recently_completed', shortLabel: 'Done' },
];

export function ActivityFilterTabs({
  active,
  onSelect,
  disabled = false,
}: ActivityFilterTabsProps) {
  return (
    <div
      className={cn(
        'inline-flex gap-0.5 rounded-full border border-border bg-muted/40 p-1',
        'max-w-full'
      )}
      role="tablist"
      aria-label="Activity feed filters"
    >
      {FILTERS.map((filter) => {
        const isActive = filter.value === active;
        return (
          <Button
            key={filter.value}
            variant={isActive ? 'default' : 'ghost'}
            size="sm"
            role="tab"
            type="button"
            aria-selected={isActive}
            aria-pressed={isActive}
            disabled={disabled}
            className={cn(
              'rounded-full px-2 text-xs font-medium transition-colors sm:px-3 sm:text-sm',
              !isActive && 'text-muted-foreground'
            )}
            onClick={() => onSelect(filter.value)}
          >
            <span className="sm:hidden">{filter.shortLabel || filter.label}</span>
            <span className="hidden sm:inline">{filter.label}</span>
          </Button>
        );
      })}
    </div>
  );
}
