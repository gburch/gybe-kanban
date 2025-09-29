import { ActivityFeedFilter } from '@/lib/api';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface ActivityFilterTabsProps {
  active: ActivityFeedFilter;
  onSelect: (filter: ActivityFeedFilter) => void;
  disabled?: boolean;
}

const FILTERS: Array<{ label: string; value: ActivityFeedFilter }> = [
  { label: 'Needs Review', value: 'need_review' },
  { label: 'In Progress', value: 'in_progress' },
  { label: 'Completed', value: 'recently_completed' },
];

export function ActivityFilterTabs({
  active,
  onSelect,
  disabled = false,
}: ActivityFilterTabsProps) {
  return (
    <div
      className={cn(
        'inline-flex gap-1 rounded-full border border-border bg-muted/40 p-1',
        'max-w-full flex-wrap sm:flex-nowrap'
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
              'rounded-full px-3 text-sm font-medium transition-colors',
              !isActive && 'text-muted-foreground'
            )}
            onClick={() => onSelect(filter.value)}
          >
            {filter.label}
          </Button>
        );
      })}
    </div>
  );
}
