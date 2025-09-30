import { useState } from 'react';
import { Bell } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { NotificationCenter } from './notification-center';
import { useActivityFeedHighPriority } from '@/stores/activityFeedStore';

interface NotificationButtonProps {
  projectId?: string | null;
  projects?: Array<{ id: string; name: string }>;
  isProjectsLoading?: boolean;
}

export function NotificationButton({
  projectId,
  projects,
  isProjectsLoading,
}: NotificationButtonProps) {
  const [open, setOpen] = useState(false);
  const highPriority = useActivityFeedHighPriority();
  const urgentCount = highPriority.length;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="relative"
          aria-label={
            urgentCount > 0
              ? `Notifications (${urgentCount} urgent)`
              : 'Notifications'
          }
        >
          <Bell className="h-4 w-4" />
          {urgentCount > 0 && (
            <span className="absolute -top-0.5 -right-0.5 flex h-4 w-4 items-center justify-center rounded-full bg-red-500 text-[9px] font-bold text-white">
              {urgentCount > 9 ? '9+' : urgentCount}
            </span>
          )}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        className="p-0 w-auto"
        align="end"
        side="bottom"
        sideOffset={8}
      >
        <NotificationCenter
          projectId={projectId}
          projects={projects}
          isProjectsLoading={isProjectsLoading}
          onClose={() => setOpen(false)}
        />
      </PopoverContent>
    </Popover>
  );
}