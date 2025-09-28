import { useCallback, useEffect } from 'react';
import { useInfiniteQuery, type InfiniteData } from '@tanstack/react-query';
import { shallow } from 'zustand/shallow';
import {
  ActivityFeedEvent,
  activityFeedApi,
  deserializeActivityFeedItem,
} from '@/lib/api';
import {
  useActivityFeedStore,
  useActivityFeedFilter,
} from '@/stores/activityFeedStore';

interface UseActivityFeedOptions {
  enabled?: boolean;
  projectId: string | null | undefined;
}

type WsChangeType = 'created' | 'updated' | 'removed';

type ActivityFeedPage = Awaited<ReturnType<typeof activityFeedApi.list>>;
type ActivityFeedQueryKey = ['activityFeed', string | null];

interface ActivityFeedWsMessage {
  type: 'activity_feed.update';
  payload: {
    event: {
      id: string;
      change_type: WsChangeType;
      event: ActivityFeedEvent | Omit<ActivityFeedEvent, 'createdAt'> | null;
    };
  };
}

const WS_RETRY_BASE_MS = 1000;
const WS_RETRY_MAX_MS = 8000;

const buildWsUrl = (projectId: string, scope: 'mine' | 'all') => {
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
  const host = window.location.host;
  const params = new URLSearchParams({ scope });
  return `${protocol}://${host}/api/projects/${projectId}/activity_feed/ws?${params.toString()}`;
};

const normalizeItem = (
  item: ActivityFeedEvent | Omit<ActivityFeedEvent, 'createdAt'>
): ActivityFeedEvent => {
  if ('createdAt' in item && item.createdAt instanceof Date) {
    return item as ActivityFeedEvent;
  }
  return deserializeActivityFeedItem(item as ActivityFeedEvent);
};

export const useActivityFeed = ({
  enabled = true,
  projectId,
}: UseActivityFeedOptions) => {
  const filter = useActivityFeedFilter();

  const { replaceEvents, setLoading, setError, setConnectionState } =
    useActivityFeedStore(
      (state) => ({
        replaceEvents: state.replaceEvents,
        setLoading: state.setLoading,
        setError: state.setError,
        setConnectionState: state.setConnectionState,
      }),
      shallow
    );

  const upsertEvent = useActivityFeedStore((state) => state.upsertEvent);
  const removeEvent = useActivityFeedStore((state) => state.removeEvent);

  const queryEnabled = enabled && Boolean(projectId);

  const query = useInfiniteQuery<
    ActivityFeedPage,
    Error,
    InfiniteData<ActivityFeedPage>,
    ActivityFeedQueryKey,
    string | null
  >({
    queryKey: ['activityFeed', projectId ?? null],
    enabled: queryEnabled,
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor ?? undefined,
    queryFn: async ({ pageParam }) => {
      if (!projectId) {
        return { events: [], nextCursor: null };
      }
      return activityFeedApi.list(projectId, {
        scope: 'mine',
        cursor: pageParam ?? null,
      });
    },
    meta: { description: 'activity feed list' },
  });

  useEffect(() => {
    const isPriming =
      query.isLoading || (query.isFetching && !query.isFetchingNextPage);
    setLoading(isPriming);
  }, [query.isLoading, query.isFetching, query.isFetchingNextPage, setLoading]);

  useEffect(() => {
    if (!query.error) {
      setError(null);
      return;
    }
    const message =
      query.error instanceof Error ? query.error.message : 'Failed to load feed';
    setError(message);
  }, [query.error, setError]);

  useEffect(() => {
    if (!query.data) {
      if (!queryEnabled) {
        replaceEvents([], null);
      }
      return;
    }

    const allEvents = query.data.pages.flatMap((page) => page.events);
    const nextCursor =
      query.data.pages[query.data.pages.length - 1]?.nextCursor ?? null;

    replaceEvents(allEvents, nextCursor);

  }, [query.data, queryEnabled, replaceEvents]);

  useEffect(() => {
    if (!projectId || !enabled) {
      setConnectionState('idle');
      return;
    }

    let ws: WebSocket | null = null;
    let retryDelay = WS_RETRY_BASE_MS;
    let retryTimer: number | null = null;
    let isUnmounted = false;

    const scheduleReconnect = () => {
      if (isUnmounted) return;
      if (retryTimer) {
        window.clearTimeout(retryTimer);
      }
      retryTimer = window.setTimeout(() => {
        retryTimer = null;
        connect();
      }, retryDelay);
      retryDelay = Math.min(retryDelay * 2, WS_RETRY_MAX_MS);
    };

    const connect = () => {
      if (isUnmounted) return;
      setConnectionState(ws ? 'reconnecting' : 'connecting');

      try {
        ws = new WebSocket(buildWsUrl(projectId, 'mine'));
      } catch (err) {
        console.warn('Failed to open activity feed websocket', err);
        setConnectionState('disconnected');
        scheduleReconnect();
        return;
      }

      ws.onopen = () => {
        retryDelay = WS_RETRY_BASE_MS;
        if (!isUnmounted) {
          setConnectionState('connected');
        }
      };

      ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data) as ActivityFeedWsMessage;
          if (message.type !== 'activity_feed.update') return;
          const change = message.payload.event;

          if (change.change_type === 'removed') {
            removeEvent(change.id);
            return;
          }

          if (change.event) {
            const normalized = normalizeItem(change.event);
            upsertEvent(normalized);
          }
        } catch (error) {
          console.error('Failed to process activity feed update', error);
        }
      };

      ws.onerror = () => {
        setConnectionState('reconnecting');
      };

      ws.onclose = () => {
        if (isUnmounted) return;
        setConnectionState('reconnecting');
        scheduleReconnect();
      };
    };

    connect();

    return () => {
      isUnmounted = true;
      setConnectionState('disconnected');
      if (retryTimer) {
        window.clearTimeout(retryTimer);
      }
      if (ws) {
        ws.close();
      }
    };
  }, [projectId, enabled, setConnectionState, removeEvent, upsertEvent, queryEnabled]);

  const loadMore = useCallback(async () => {
    if (!queryEnabled) return;
    await query.fetchNextPage();
  }, [queryEnabled, query]);

  return {
    loadMore,
    filter,
    setFilter: useActivityFeedStore((state) => state.setFilter),
    markAsHandled: useActivityFeedStore((state) => state.markAsHandled),
    hasMore: query.hasNextPage ?? false,
    isFetchingNextPage: query.isFetchingNextPage,
    refetch: query.refetch,
  };
};
