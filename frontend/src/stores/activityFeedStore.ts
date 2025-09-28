import { create } from 'zustand';
import { ActivityFeedFilter, type ActivityFeedEvent } from '@/lib/api';

export type ActivityFeedConnectionState =
  | 'idle'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'disconnected';

type ActivityFeedState = {
  filter: ActivityFeedFilter;
  events: ActivityFeedEvent[];
  dismissedHighPriority: Set<string>;
  nextCursor: string | null;
  hasMore: boolean;
  isLoading: boolean;
  error: string | null;
  connectionState: ActivityFeedConnectionState;
  connectionAttempts: number;
  hasConnectedOnce: boolean;

  setFilter: (filter: ActivityFeedFilter) => void;
  setLoading: (isLoading: boolean) => void;
  setError: (error: string | null) => void;
  setConnectionState: (state: ActivityFeedConnectionState) => void;

  replaceEvents: (
    events: ActivityFeedEvent[],
    nextCursor: string | null
  ) => void;
  appendEvents: (
    events: ActivityFeedEvent[],
    nextCursor: string | null
  ) => void;
  upsertEvent: (event: ActivityFeedEvent) => void;
  removeEvent: (eventId: string) => void;
  markAsHandled: (eventId: string) => void;
  clear: () => void;
};

const sortEvents = (
  events: ActivityFeedEvent[],
  dismissed: Set<string>
): ActivityFeedEvent[] => {
  const highPriority: ActivityFeedEvent[] = [];
  const normalPriority: ActivityFeedEvent[] = [];

  for (const event of events) {
    if (event.actionRequired && !dismissed.has(event.id)) {
      highPriority.push(event);
    } else {
      normalPriority.push(event);
    }
  }

  const byCreatedAtDesc = (a: ActivityFeedEvent, b: ActivityFeedEvent) =>
    b.createdAt.getTime() - a.createdAt.getTime();

  highPriority.sort(byCreatedAtDesc);
  normalPriority.sort(byCreatedAtDesc);

  return [...highPriority, ...normalPriority];
};

const dedupeEvents = (events: ActivityFeedEvent[]): ActivityFeedEvent[] => {
  const map = new Map<string, ActivityFeedEvent>();
  for (const event of events) {
    map.set(event.id, event);
  }
  return Array.from(map.values());
};

export const useActivityFeedStore = create<ActivityFeedState>((set, get) => ({
  filter: 'need_review',
  events: [],
  dismissedHighPriority: new Set(),
  nextCursor: null,
  hasMore: false,
  isLoading: false,
  error: null,
  connectionState: 'idle',
  connectionAttempts: 0,
  hasConnectedOnce: false,

  setFilter: (filter) => {
    if (get().filter === filter) return;
    set({
      filter,
      dismissedHighPriority: new Set(),
      nextCursor: null,
      hasMore: false,
      events: [],
      error: null,
      connectionState: 'idle',
      connectionAttempts: 0,
      hasConnectedOnce: false,
    });
  },

  setLoading: (isLoading) => set({ isLoading }),
  setError: (error) => set({ error }),
  setConnectionState: (state) =>
    set((current) => ({
      connectionState: state,
      connectionAttempts:
        state === 'connecting'
          ? current.connectionAttempts + 1
          : state === 'idle'
            ? 0
            : current.connectionAttempts,
      hasConnectedOnce:
        state === 'connected' ? true : current.hasConnectedOnce,
    })),

  replaceEvents: (events, nextCursor) => {
    const dismissed = get().dismissedHighPriority;
    const deduped = dedupeEvents(events);
    set({
      events: sortEvents(deduped, dismissed),
      nextCursor,
      hasMore: Boolean(nextCursor),
    });
  },

  appendEvents: (events, nextCursor) => {
    if (!events.length) {
      set({ nextCursor, hasMore: Boolean(nextCursor) });
      return;
    }
    const current = get().events;
    const merged = dedupeEvents([...current, ...events]);
    const dismissed = get().dismissedHighPriority;
    set({
      events: sortEvents(merged, dismissed),
      nextCursor,
      hasMore: Boolean(nextCursor),
    });
  },

  upsertEvent: (event) => {
    const current = get().events;
    const merged = dedupeEvents([event, ...current]);
    const dismissed = get().dismissedHighPriority;
    set({ events: sortEvents(merged, dismissed) });
  },

  removeEvent: (eventId) => {
    const filtered = get().events.filter((event) => event.id !== eventId);
    const dismissed = new Set(get().dismissedHighPriority);
    dismissed.delete(eventId);
    set({
      events: filtered,
      dismissedHighPriority: dismissed,
    });
  },

  markAsHandled: (eventId) => {
    const dismissed = new Set(get().dismissedHighPriority);
    dismissed.add(eventId);
    const reordered = sortEvents(get().events, dismissed);
    set({
      dismissedHighPriority: dismissed,
      events: reordered,
    });
  },

  clear: () =>
    set({
      events: [],
      nextCursor: null,
      hasMore: false,
      error: null,
      isLoading: false,
      connectionState: 'idle',
      connectionAttempts: 0,
      hasConnectedOnce: false,
      dismissedHighPriority: new Set(),
    }),
}));

export const useActivityFeedEvents = () =>
  useActivityFeedStore((state) => state.events);

export const useActivityFeedHighPriority = () =>
  useActivityFeedStore((state) =>
    state.events.filter(
      (event) => event.actionRequired && !state.dismissedHighPriority.has(event.id)
    )
  );

export const useActivityFeedFilter = () =>
  useActivityFeedStore((state) => state.filter);

export const useActivityFeedConnection = () =>
  useActivityFeedStore((state) => state.connectionState);

export const useActivityFeedConnectionMeta = () =>
  useActivityFeedStore((state) => ({
    connectionState: state.connectionState,
    connectionAttempts: state.connectionAttempts,
    hasConnectedOnce: state.hasConnectedOnce,
  }));

export const useActivityFeedStatus = () =>
  useActivityFeedStore((state) => ({
    isLoading: state.isLoading,
    error: state.error,
    hasMore: state.hasMore,
    nextCursor: state.nextCursor,
  }));
