import {
  useEffect,
  useMemo,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type TouchEvent as ReactTouchEvent,
  useRef,
} from 'react';
import { useNavigate } from 'react-router-dom';
import { AlertTriangle, ArrowUpRight, CheckCircle2, Loader2, WifiOff, X } from 'lucide-react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { ActivityFeedEvent, ActivityFeedFilter } from '@/lib/api';
import { cn } from '@/lib/utils';
import { ActivityFilterTabs } from '@/components/home/ActivityFilterTabs';
import {
  useActivityFeedEvents,
  useActivityFeedHighPriority,
  useActivityFeedStatus,
  useActivityFeedFilter,
  useActivityFeedConnectionMeta,
} from '@/stores/activityFeedStore';
import { useActivityFeed } from '@/hooks/useActivityFeed';
import { trackAnalyticsEvent } from '@/lib/analytics';
import { CompactCodexUsage } from './compact-codex-usage';
import { CompactClaudeCodeUsage } from './compact-claude-code-usage';

const STORAGE_KEY = 'vk.activity_feed.filter';
const TOUCH_TAP_MOVE_THRESHOLD = 10;
const TOUCH_ACTIVATION_CLEAR_DELAY_MS = 400;

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

export interface NotificationCenterProps {
  projectId?: string | null;
  projects?: ProjectDescriptor[];
  onEventDismiss?: (eventId: string) => void;
  isProjectsLoading?: boolean;
  onClose?: () => void;
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

export function NotificationCenter({
  projectId,
  projects,
  onEventDismiss,
  isProjectsLoading = false,
  onClose,
}: NotificationCenterProps) {
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
      onClose?.();
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

  const supportsPointerEvents =
    typeof window !== 'undefined' && window.PointerEvent != null;

  const touchStartCoordinates = useRef<
    Map<number | string, { x: number; y: number }>
  >(new Map());
  const touchActivatedEvents = useRef<Set<string>>(new Set());

  const markEventActivatedByTouch = (eventId: string) => {
    touchActivatedEvents.current.add(eventId);
    window.setTimeout(() => {
      touchActivatedEvents.current.delete(eventId);
    }, TOUCH_ACTIVATION_CLEAR_DELAY_MS);
  };

  const handleCardPointerDown = (
    pointerEvent: ReactPointerEvent<HTMLElement>,
    event: ActivityFeedEvent
  ) => {
    if (!event.cta?.href) return;
    if (!pointerEvent.isPrimary) return;
    if (pointerEvent.pointerType !== 'touch') return;

    touchStartCoordinates.current.set(pointerEvent.pointerId, {
      x: pointerEvent.clientX,
      y: pointerEvent.clientY,
    });
  };

  const handleCardPointerUp = (
    pointerEvent: ReactPointerEvent<HTMLElement>,
    event: ActivityFeedEvent
  ) => {
    if (!event.cta?.href) return;
    if (!pointerEvent.isPrimary) return;
    if (pointerEvent.pointerType !== 'touch') return;

    const start = touchStartCoordinates.current.get(pointerEvent.pointerId);
    touchStartCoordinates.current.delete(pointerEvent.pointerId);
    if (!start) return;

    const deltaX = Math.abs(pointerEvent.clientX - start.x);
    const deltaY = Math.abs(pointerEvent.clientY - start.y);
    if (
      deltaX > TOUCH_TAP_MOVE_THRESHOLD ||
      deltaY > TOUCH_TAP_MOVE_THRESHOLD
    ) {
      return;
    }

    markEventActivatedByTouch(event.id);
    handleEventActivation(event);
  };

  const handleCardPointerCancel = (
    pointerEvent: ReactPointerEvent<HTMLElement>
  ) => {
    if (pointerEvent.pointerType !== 'touch') return;
    touchStartCoordinates.current.delete(pointerEvent.pointerId);
  };

  const handleCardTouchStart = (
    touchEvent: ReactTouchEvent<HTMLElement>,
    event: ActivityFeedEvent
  ) => {
    if (supportsPointerEvents) return;
    if (!event.cta?.href) return;
    if (touchEvent.touches.length !== 1) return;
    const touch = touchEvent.touches[0];
    touchStartCoordinates.current.set(event.id, {
      x: touch.clientX,
      y: touch.clientY,
    });
  };

  const handleCardTouchEnd = (
    touchEvent: ReactTouchEvent<HTMLElement>,
    event: ActivityFeedEvent
  ) => {
    if (supportsPointerEvents) return;
    if (!event.cta?.href) return;
    const start = touchStartCoordinates.current.get(event.id);
    touchStartCoordinates.current.delete(event.id);
    if (!start) return;

    const changedTouch = touchEvent.changedTouches[0];
    if (!changedTouch) return;

    const deltaX = Math.abs(changedTouch.clientX - start.x);
    const deltaY = Math.abs(changedTouch.clientY - start.y);
    if (
      deltaX > TOUCH_TAP_MOVE_THRESHOLD ||
      deltaY > TOUCH_TAP_MOVE_THRESHOLD
    ) {
      return;
    }

    markEventActivatedByTouch(event.id);
    handleEventActivation(event);
  };

  const handleCardTouchCancel = (
    _touchEvent: ReactTouchEvent<HTMLElement>,
    event: ActivityFeedEvent
  ) => {
    if (supportsPointerEvents) return;
    touchStartCoordinates.current.delete(event.id);
  };

  const handleCardClick = (event: ActivityFeedEvent) => {
    if (!event.cta?.href) return;

    if (touchActivatedEvents.current.has(event.id)) {
      touchActivatedEvents.current.delete(event.id);
      return;
    }

    handleEventActivation(event);
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

  // Limit display to 5 events in the popover
  const displayedEvents = filteredEvents.slice(0, 5);

  return (
    <div className="w-[400px] max-h-[600px] flex flex-col overflow-hidden">
      {/* Header */}
      <div className="px-3 pb-3 pt-1 border-b border-border shrink-0">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <h3 className="font-semibold text-sm">Notifications</h3>
            {urgentCount > 0 && (
              <Badge variant="destructive" className="text-[10px] px-1.5 py-0 h-4">
                {urgentCount}
              </Badge>
            )}
          </div>
        </div>
      </div>

      {/* Claude Code Limits */}
      <CompactClaudeCodeUsage />

      {/* Codex Limits */}
      <CompactCodexUsage />

      {/* Filter Tabs */}
      <div className="px-3 py-3 border-b border-border shrink-0 flex justify-center">
        <ActivityFilterTabs
          active={filter}
          onSelect={handleFilterChange}
          disabled={status.isLoading && events.length === 0}
        />
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-3 py-3 space-y-3 min-h-0">
        {showNoProjectsBanner && (
          <div className="text-xs text-muted-foreground text-center py-4">
            No projects available yet.
          </div>
        )}

        {status.error && (
          <Alert variant="destructive" className="py-2">
            <AlertTriangle className="h-3 w-3" />
            <AlertDescription className="text-xs">{status.error}</AlertDescription>
          </Alert>
        )}

        {showReconnect && (
          <Alert variant="default" className="border border-amber-500/30 bg-amber-50/70 dark:bg-amber-900/20 py-2">
            <WifiOff className="h-3 w-3" />
            <AlertDescription className="text-xs">
              Reconnecting to live updates…
            </AlertDescription>
          </Alert>
        )}

        {isSkeleton && (
          <div className="space-y-2" aria-hidden="true">
            {[0, 1, 2].map((key) => (
              <div
                key={key}
                className="flex items-start gap-2 rounded-lg border border-border/50 bg-muted/30 p-2"
              >
                <div className="h-8 w-8 animate-pulse rounded-full bg-muted" />
                <div className="flex-1 space-y-1.5">
                  <div className="h-3 w-2/3 animate-pulse rounded bg-muted" />
                  <div className="h-2 w-full animate-pulse rounded bg-muted" />
                </div>
              </div>
            ))}
          </div>
        )}

        {!isSkeleton && (
          <ul className="space-y-2" aria-live="polite" aria-busy={status.isLoading}>
            {displayedEvents.map((event) => {
              const urgent = isHighPriority(event);
              const projectName = event.projectId
                ? projectLookup.get(event.projectId) ?? null
                : null;
              const isClickable = Boolean(event.cta?.href);
              return (
                <li key={event.id}>
                  <article
                    className={cn(
                      'group flex gap-2 rounded-lg border border-border/70 bg-card p-2 transition-colors',
                      isClickable &&
                        'cursor-pointer hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                      urgent &&
                        'border-amber-500/60 bg-amber-50/50 dark:border-amber-400/40 dark:bg-amber-950/20'
                    )}
                    tabIndex={isClickable ? 0 : -1}
                    role={isClickable ? 'button' : undefined}
                    aria-label={event.headline}
                    onPointerDown={
                      isClickable
                        ? (pointerEvent) =>
                            handleCardPointerDown(pointerEvent, event)
                        : undefined
                    }
                    onPointerUp={
                      isClickable
                        ? (pointerEvent) =>
                            handleCardPointerUp(pointerEvent, event)
                        : undefined
                    }
                    onPointerCancel={
                      isClickable
                        ? (pointerEvent) =>
                            handleCardPointerCancel(pointerEvent)
                        : undefined
                    }
                    onTouchStart={
                      isClickable
                        ? (touchEvent) =>
                            handleCardTouchStart(touchEvent, event)
                        : undefined
                    }
                    onTouchEnd={
                      isClickable
                        ? (touchEvent) =>
                            handleCardTouchEnd(touchEvent, event)
                        : undefined
                    }
                    onTouchCancel={
                      isClickable
                        ? (touchEvent) =>
                            handleCardTouchCancel(touchEvent, event)
                        : undefined
                    }
                    onClick={
                      isClickable
                        ? () => handleCardClick(event)
                        : undefined
                    }
                    onKeyDown={
                      isClickable
                        ? (keyboardEvent) =>
                            handleCardKeyDown(keyboardEvent, event)
                        : undefined
                    }
                  >
                    <div className="relative flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary/10 text-xs font-semibold text-primary">
                      {getInitial(event.headline)}
                      {urgent && (
                        <span className="absolute -top-0.5 -right-0.5 inline-flex h-3.5 w-3.5 items-center justify-center rounded-full bg-amber-500 text-[9px] font-bold text-amber-50">
                          !
                        </span>
                      )}
                    </div>
                    <div className="flex flex-1 flex-col gap-1 min-w-0">
                      <div className="flex flex-wrap items-center gap-1">
                        <h4 className="text-xs font-semibold leading-tight truncate">
                          {event.headline}
                        </h4>
                        {projectName && (
                          <Badge variant="outline" className="text-[9px] px-1 py-0 h-4">
                            {projectName}
                          </Badge>
                        )}
                      </div>
                      {event.summary && (
                        <p className="text-[10px] text-muted-foreground line-clamp-2">
                          {event.summary}
                        </p>
                      )}
                      <div className="flex items-center gap-2 text-[9px] text-muted-foreground">
                        <span aria-label={event.createdAt.toLocaleString()}>
                          {formatRelativeTime(event.createdAt)}
                        </span>
                      </div>
                    </div>
                    <div className="flex shrink-0 flex-col items-end gap-1">
                      {event.cta && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6"
                          onClick={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                            handleCtaClick(event);
                          }}
                          onPointerDown={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                          }}
                          onPointerUp={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                          }}
                          onPointerCancel={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                          }}
                          onTouchStart={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                          }}
                          onTouchEnd={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                          }}
                          onTouchCancel={(ctaEvent) => {
                            ctaEvent.stopPropagation();
                          }}
                          asChild
                        >
                          <a href={event.cta.href}>
                            <ArrowUpRight className="h-3 w-3" />
                          </a>
                        </Button>
                      )}
                      {onEventDismiss && urgent && (
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                            handleDismiss(event.id);
                          }}
                          onPointerDown={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                          }}
                          onPointerUp={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                          }}
                          onPointerCancel={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                          }}
                          onTouchStart={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                          }}
                          onTouchEnd={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                          }}
                          onTouchCancel={(dismissEvent) => {
                            dismissEvent.stopPropagation();
                          }}
                          className="h-6 w-6"
                          aria-label="Dismiss"
                        >
                          <X className="h-3 w-3" />
                        </Button>
                      )}
                    </div>
                  </article>
                </li>
              );
            })}
          </ul>
        )}

        {isEmpty && (
          <div className="flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-border/60 bg-muted/30 px-4 py-8 text-center">
            <div className="flex h-8 w-8 items-center justify-center rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
              <CheckCircle2 className="h-4 w-4" />
            </div>
            <div className="space-y-1">
              <h4 className="text-xs font-semibold">All caught up</h4>
              <p className="text-[10px] text-muted-foreground">
                No activity in this filter
              </p>
            </div>
          </div>
        )}

        {hasMore && displayedEvents.length >= 5 && (
          <div className="flex justify-center pt-2">
            <Button
              variant="outline"
              size="sm"
              onClick={loadMore}
              disabled={isFetchingNextPage}
              className="text-xs h-7"
            >
              {isFetchingNextPage ? (
                <span className="inline-flex items-center gap-1">
                  <Loader2 className="h-3 w-3 animate-spin" /> Loading…
                </span>
              ) : (
                'Load more'
              )}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
