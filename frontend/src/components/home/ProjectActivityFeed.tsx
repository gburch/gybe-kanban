import { useEffect, useMemo, type KeyboardEvent } from 'react';
import {
  AlertTriangle,
  ArrowUpRight,
  CheckCircle2,
  Loader2,
  WifiOff,
  X,
} from 'lucide-react';

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { ActivityFeedEvent, ActivityFeedFilter } from '@/lib/api';
import { cn } from '@/lib/utils';
import { ActivityFilterTabs } from './ActivityFilterTabs';
import {
  useActivityFeedEvents,
  useActivityFeedHighPriority,
  useActivityFeedStatus,
  useActivityFeedFilter,
  useActivityFeedConnectionMeta,
} from '@/stores/activityFeedStore';
import { useActivityFeed } from '@/hooks/useActivityFeed';
import { trackAnalyticsEvent } from '@/lib/analytics';
import { useNavigate } from 'react-router-dom';

const STORAGE_KEY = 'vk.activity_feed.filter';

const formatRelativeTime = (date: Date): string => {
  const now = Date.now();
  const diffMs = now - date.getTime();
  if (Number.isNaN(diffMs)) return '';

  if (diffMs < 0) {
    return 'just now';
  }

  const diffSeconds = Math.floor(diffMs / 1000);
  if (diffSeconds < 60) {
    return `${diffSeconds}s ago`;
  }

  const diffMinutes = Math.floor(diffSeconds / 60);
  if (diffMinutes < 60) {
    return `${diffMinutes}m ago`;
  }

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 24) {
    return `${diffHours}h ago`;
  }

  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 7) {
    return `${diffDays}d ago`;
  }

  return date.toLocaleDateString();
};

const getInitial = (text: string | undefined) => {
  if (!text) return '•';
  const trimmed = text.trim();
  if (!trimmed) return '•';
  return trimmed.charAt(0).toUpperCase();
};

interface ProjectDescriptor {
  id: string;
  name: string;
}

export interface ProjectActivityFeedProps {
  projectId?: string | null;
  projects?: ProjectDescriptor[];
  onEventDismiss?: (eventId: string) => void;
  className?: string;
  isProjectsLoading?: boolean;
}

const isHighPriority = (event: ActivityFeedEvent) => event.actionRequired;

const shouldShowReconnectBanner = (state: string, hasTried: boolean) => {
  if (!hasTried) return false;
  return state === 'reconnecting' || state === 'disconnected';
};

const REVIEW_STATUSES = new Set([
  'inreview',
  'needs_review',
  'need_review',
  'pending_review',
]);

const IN_PROGRESS_STATUSES = new Set([
  'inprogress',
  'todo',
  'executorrunning',
  'executorwaiting',
  'running',
  'queued',
]);

const COMPLETED_STATUSES = new Set([
  'done',
  'cancelled',
  'completed',
  'succeeded',
  'executorcomplete',
]);

const extractStatusToken = (summary?: string | null) => {
  if (!summary) return null;
  const normalized = summary.toLowerCase();
  const statusMatch = normalized.match(/status:\s*([a-z_]+)/i);
  if (statusMatch?.[1]) {
    return statusMatch[1];
  }

  const attemptMatch = normalized.match(/attempt state:\s*([a-z_]+)/i);
  if (attemptMatch?.[1]) {
    return attemptMatch[1];
  }

  return null;
};

const categorizeEvent = (event: ActivityFeedEvent): ActivityFeedFilter => {
  const status = extractStatusToken(event.summary);

  if (status) {
    if (REVIEW_STATUSES.has(status)) {
      return 'need_review';
    }
    if (COMPLETED_STATUSES.has(status)) {
      return 'recently_completed';
    }
    if (IN_PROGRESS_STATUSES.has(status)) {
      return 'in_progress';
    }
  }

  if (event.actionRequired || event.urgencyScore >= 70) {
    return 'need_review';
  }
  if (event.urgencyScore >= 40) {
    return 'in_progress';
  }
  return 'recently_completed';
};

export function ProjectActivityFeed({
  projectId,
  projects,
  onEventDismiss,
  className,
  isProjectsLoading = false,
}: ProjectActivityFeedProps) {
  const events = useActivityFeedEvents();
  const highPriority = useActivityFeedHighPriority();
  const status = useActivityFeedStatus();
  const filter = useActivityFeedFilter();
  const navigate = useNavigate();
  const { connectionState, connectionAttempts, hasConnectedOnce } =
    useActivityFeedConnectionMeta();
  const urgentCount = highPriority.length;

  const resolvedProjectId = projectId ?? projects?.[0]?.id ?? null;
  const showNoProjectsBanner =
    !isProjectsLoading && !projectId && (!!projects && projects.length === 0);

  const projectLookup = useMemo(() => {
    if (!projects) return new Map<string, string>();
    return new Map(projects.map((proj) => [proj.id, proj.name]));
  }, [projects]);

  const { loadMore, setFilter, markAsHandled, hasMore, isFetchingNextPage } =
    useActivityFeed({
      projectId: resolvedProjectId,
      enabled: Boolean(resolvedProjectId),
    });

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const stored = window.localStorage.getItem(STORAGE_KEY) as
      | ActivityFeedFilter
      | null;
    if (
      stored &&
      ['in_progress', 'need_review', 'recently_completed'].includes(stored)
    ) {
      setFilter(stored as ActivityFeedFilter);
    }
  }, [setFilter]);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(STORAGE_KEY, filter);
    } catch (error) {
      console.warn('Failed to persist activity feed scope', error);
    }
  }, [filter]);

  const handleFilterChange = (value: ActivityFeedFilter) => {
    setFilter(value);
  };

  const trackEventView = (event: ActivityFeedEvent) => {
    trackAnalyticsEvent('activity_feed.view_item', {
      eventId: event.id,
      filter,
    });
  };

  const navigateToEvent = (event: ActivityFeedEvent) => {
    if (!event.cta?.href) return;
    const href = event.cta.href;

    if (href.startsWith('/')) {
      navigate(href);
      return;
    }

    if (typeof window !== 'undefined') {
      window.location.assign(href);
    }
  };

  const handleEventActivation = (event: ActivityFeedEvent) => {
    if (!event.cta?.href) return;
    trackEventView(event);
    navigateToEvent(event);
  };

  const handleCardKeyDown = (
    keyboardEvent: KeyboardEvent<HTMLElement>,
    event: ActivityFeedEvent
  ) => {
    if (!event.cta?.href) return;
    if (keyboardEvent.key === 'Enter' || keyboardEvent.key === ' ') {
      keyboardEvent.preventDefault();
      handleEventActivation(event);
    }
  };

  const handleCtaClick = (event: ActivityFeedEvent) => {
    trackEventView(event);
  };

  const handleDismiss = (eventId: string) => {
    markAsHandled(eventId);
    if (onEventDismiss) {
      onEventDismiss(eventId);
    }
  };

  const filteredEvents = useMemo(() => {
    return events.filter((event) => categorizeEvent(event) === filter);
  }, [events, filter]);

  const isSkeleton = status.isLoading && events.length === 0;
  const isEmpty =
    Boolean(resolvedProjectId) &&
    !status.isLoading &&
    filteredEvents.length === 0 &&
    !status.error;
  const attemptedConnection = connectionAttempts > 1;
  const showReconnect =
    (hasConnectedOnce || attemptedConnection) &&
    shouldShowReconnectBanner(connectionState, attemptedConnection);

  return (
    <Card className={cn('relative overflow-hidden', className)}>
      <CardHeader className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between sm:gap-6">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-3">
            <CardTitle className="text-xl font-semibold">
              Recent Activity
            </CardTitle>
            {urgentCount > 0 ? (
              <Badge variant="destructive" className="uppercase">
                {urgentCount} urgent
              </Badge>
            ) : null}
          </div>
          <p className="text-sm text-muted-foreground">
            Stay on top of project updates without leaving home base.
          </p>
        </div>
        <div className="flex w-full justify-start sm:w-auto sm:justify-end">
          <ActivityFilterTabs
            active={filter}
            onSelect={handleFilterChange}
            disabled={status.isLoading && events.length === 0}
          />
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {showNoProjectsBanner ? (
          <div className="flex items-center justify-between gap-6 rounded-lg border border-dashed border-border p-6 text-sm text-muted-foreground">
            <span>No projects available yet. Create a project to unlock activity.</span>
          </div>
        ) : null}

        {status.error ? (
          <Alert variant="destructive">
            <AlertTriangle className="h-4 w-4" />
            <AlertDescription>{status.error}</AlertDescription>
          </Alert>
        ) : null}

        {showReconnect ? (
          <Alert variant="default" className="border border-amber-500/30 bg-amber-50/70 dark:bg-amber-900/20">
            <WifiOff className="h-4 w-4" />
            <AlertDescription>
              Reconnecting to live updates… New activity will appear once we
              restore the connection.
            </AlertDescription>
          </Alert>
        ) : null}

        {isSkeleton ? (
          <div className="space-y-3" aria-hidden="true">
            {[0, 1, 2].map((key) => (
              <div
                key={key}
                className="flex items-start gap-3 rounded-lg border border-border/50 bg-muted/30 p-4"
              >
                <div className="h-10 w-10 animate-pulse rounded-full bg-muted" />
                <div className="flex-1 space-y-2">
                  <div className="h-4 w-2/3 animate-pulse rounded bg-muted" />
                  <div className="h-3 w-full animate-pulse rounded bg-muted" />
                  <div className="h-3 w-1/2 animate-pulse rounded bg-muted" />
                </div>
                <div className="h-6 w-16 animate-pulse rounded bg-muted" />
              </div>
            ))}
          </div>
        ) : null}

        {!isSkeleton ? (
          <ul
            className="space-y-3"
            aria-live="polite"
            aria-busy={status.isLoading}
          >
            {filteredEvents.map((event) => {
              const urgent = isHighPriority(event);
              const projectName = event.projectId
                ? projectLookup.get(event.projectId) ?? null
                : null;
              const isClickable = Boolean(event.cta?.href);
              return (
                <li key={event.id}>
                  <article
                    className={cn(
                      'group flex gap-3 rounded-lg border border-border/70 bg-card p-4 transition-colors focus-within:ring-2 focus-within:ring-ring focus-within:ring-offset-2',
                      isClickable &&
                        'cursor-pointer hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2',
                      urgent &&
                        'border-amber-500/60 bg-amber-50/50 dark:border-amber-400/40 dark:bg-amber-950/20'
                    )}
                    tabIndex={isClickable ? 0 : -1}
                    role={isClickable ? 'button' : undefined}
                    aria-label={event.headline}
                    onClick={
                      isClickable
                        ? () => handleEventActivation(event)
                        : undefined
                    }
                    onKeyDown={
                      isClickable
                        ? (keyboardEvent) =>
                            handleCardKeyDown(keyboardEvent, event)
                        : undefined
                    }
                  >
                    <div className="relative flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-sm font-semibold text-primary">
                      {getInitial(event.headline)}
                      {urgent ? (
                        <span className="absolute -top-1 -right-1 inline-flex h-5 w-5 items-center justify-center rounded-full bg-amber-500 text-[10px] font-bold text-amber-50">
                          !
                        </span>
                      ) : null}
                    </div>
                    <div className="flex flex-1 flex-col gap-2">
                      <div className="flex flex-wrap items-center gap-2">
                        <h3 className="text-sm font-semibold leading-snug">
                          {event.headline}
                        </h3>
                        {urgent ? (
                          <Badge variant="destructive" className="uppercase">
                            Action required
                          </Badge>
                        ) : null}
                        {projectName ? (
                          <Badge variant="outline">{projectName}</Badge>
                        ) : null}
                      </div>
                      {event.summary ? (
                        <p className="text-sm text-muted-foreground">
                          {event.summary}
                        </p>
                      ) : null}
                      <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                        <span aria-label={event.createdAt.toLocaleString()}>
                          {formatRelativeTime(event.createdAt)}
                        </span>
                        <span className="hidden h-1 w-1 rounded-full bg-muted-foreground/40 sm:block" />
                        <span className="uppercase tracking-wide text-muted-foreground/80">
                          urgency {event.urgencyScore}
                        </span>
                      </div>
                    </div>
                    <div className="flex shrink-0 flex-col items-end gap-2">
                      {event.cta ? (
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                            handleCtaClick(event);
                          }}
                          asChild
                        >
                          <a href={event.cta.href} className="inline-flex items-center gap-2">
                            {event.cta.label}
                            <ArrowUpRight className="h-4 w-4" />
                          </a>
                        </Button>
                      ) : null}
                      {onEventDismiss && urgent ? (
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                            handleDismiss(event.id);
                          }}
                          className="h-8 w-8"
                          aria-label="Dismiss from high priority"
                        >
                          <X className="h-4 w-4" />
                        </Button>
                      ) : null}
                    </div>
                  </article>
                </li>
              );
            })}
          </ul>
        ) : null}

        {isEmpty ? (
          <div className="flex flex-col items-center justify-center gap-4 rounded-xl border border-dashed border-border/60 bg-muted/30 px-6 py-12 text-center">
            <div className="flex h-12 w-12 items-center justify-center rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
              <CheckCircle2 className="h-6 w-6" />
            </div>
            <div className="space-y-2">
              <h3 className="text-base font-semibold">You're all caught up</h3>
              <p className="max-w-sm text-sm text-muted-foreground">
                Activity that matters to you will show up here. Tweak the filters to broaden the feed.
              </p>
            </div>
          </div>
        ) : null}

        {hasMore ? (
          <div className="flex justify-center">
            <Button
              variant="outline"
              onClick={loadMore}
              disabled={isFetchingNextPage}
            >
              {isFetchingNextPage ? (
                <span className="inline-flex items-center gap-2">
                  <Loader2 className="h-4 w-4 animate-spin" /> Loading…
                </span>
              ) : (
                'Load more'
              )}
            </Button>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
