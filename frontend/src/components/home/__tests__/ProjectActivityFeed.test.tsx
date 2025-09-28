import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ProjectActivityFeed } from '../ProjectActivityFeed';
import { useActivityFeedStore } from '@/stores/activityFeedStore';
import type { ActivityFeedEvent } from '@/lib/api';

const mockUseActivityFeed = vi.fn();

const { defaultFeedApi } = vi.hoisted(() => ({
  defaultFeedApi: {
    loadMore: vi.fn(),
    setFilter: vi.fn(),
    markAsHandled: vi.fn(),
    hasMore: false,
    isFetchingNextPage: false,
  },
}));

const { navigateMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
}));

vi.mock('@/hooks/useActivityFeed', () => ({
  useActivityFeed: (options: unknown) =>
    mockUseActivityFeed(options) ?? defaultFeedApi,
}));

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>(
    'react-router-dom'
  );
  return {
    ...actual,
    useNavigate: () => navigateMock,
  };
});

const { trackAnalyticsEvent } = vi.hoisted(() => ({
  trackAnalyticsEvent: vi.fn(),
}));

vi.mock('@/lib/analytics', () => ({
  trackAnalyticsEvent,
}));

const resetStore = () => {
  const state = useActivityFeedStore.getState();
  useActivityFeedStore.setState({
    ...state,
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
  });
};

const stubEvent = (overrides: Partial<ActivityFeedEvent> = {}): ActivityFeedEvent => ({
  id: overrides.id ?? crypto.randomUUID(),
  headline: overrides.headline ?? 'New deployment ready',
  summary: overrides.summary ?? 'Deployment completed successfully',
  cta: overrides.cta ?? { label: 'View', href: '/deployments/1' },
  urgencyScore: overrides.urgencyScore ?? 82,
  actionRequired: overrides.actionRequired ?? true,
  createdAt: overrides.createdAt ?? new Date('2025-09-20T12:00:00Z'),
});

const ensurePointerEvent = () => {
  if (typeof window.PointerEvent !== 'undefined') {
    return;
  }

  class MockPointerEvent extends Event {
    pointerId: number;
    pointerType: string;
    clientX: number;
    clientY: number;
    isPrimary: boolean;

    constructor(type: string, init?: PointerEventInit) {
      super(type, init);
      this.pointerId = init?.pointerId ?? 0;
      this.pointerType = init?.pointerType ?? 'mouse';
      this.clientX = init?.clientX ?? 0;
      this.clientY = init?.clientY ?? 0;
      this.isPrimary = init?.isPrimary ?? true;
    }
  }

  Object.defineProperty(window, 'PointerEvent', {
    configurable: true,
    writable: true,
    value: MockPointerEvent,
  });
};

beforeEach(() => {
  resetStore();
  navigateMock.mockReset();
  defaultFeedApi.loadMore.mockReset();
  defaultFeedApi.setFilter.mockReset();
  defaultFeedApi.markAsHandled.mockReset();
  mockUseActivityFeed.mockReturnValue(defaultFeedApi);
  trackAnalyticsEvent.mockReset();
  if (!navigator.sendBeacon) {
    Object.defineProperty(navigator, 'sendBeacon', {
      writable: true,
      value: vi.fn(),
    });
  }
  localStorage.clear();
});

afterEach(() => {
  resetStore();
  mockUseActivityFeed.mockReset();
});

describe('ProjectActivityFeed', () => {
  it('shows skeleton while loading initial data', () => {
    useActivityFeedStore.setState((state) => ({
      ...state,
      isLoading: true,
      events: [],
    }));

    const { container } = render(
      <ProjectActivityFeed projectId="proj-1" isProjectsLoading={false} />
    );

    expect(container.querySelectorAll('.animate-pulse').length).toBeGreaterThan(0);
  });

  it('shows empty state when no events are returned', () => {
    useActivityFeedStore.setState((state) => ({
      ...state,
      isLoading: false,
      events: [],
      error: null,
    }));

    render(
      <ProjectActivityFeed
        projectId="proj-1"
        isProjectsLoading={false}
        onEventDismiss={() => undefined}
      />
    );

    expect(screen.getByText(/you're all caught up/i)).toBeInTheDocument();
  });

  it('renders event with CTA and tracks analytics on click', async () => {
    const user = userEvent.setup();
    const event = stubEvent();

    useActivityFeedStore.getState().replaceEvents([event], null);

    render(
      <ProjectActivityFeed
        projectId="proj-1"
        isProjectsLoading={false}
        onEventDismiss={() => undefined}
      />
    );

    const cta = screen.getByRole('link', { name: /view/i });
    await user.click(cta);

    expect(trackAnalyticsEvent).toHaveBeenCalledWith(
      'activity_feed.view_item',
      expect.objectContaining({ eventId: event.id })
    );
  });

  it('navigates when the activity card is clicked', async () => {
    const user = userEvent.setup();
    const event = stubEvent();

    useActivityFeedStore.getState().replaceEvents([event], null);

    render(
      <ProjectActivityFeed
        projectId="proj-1"
        isProjectsLoading={false}
        onEventDismiss={() => undefined}
      />
    );

    const card = screen.getByRole('button', { name: event.headline });
    await user.click(card);

    expect(trackAnalyticsEvent).toHaveBeenCalledWith(
      'activity_feed.view_item',
      expect.objectContaining({ eventId: event.id })
    );
    expect(navigateMock).toHaveBeenCalledWith(event.cta?.href);
  });

  it('navigates when the activity card is tapped as a pointer event', () => {
    ensurePointerEvent();

    const event = stubEvent({ id: 'touch-pointer' });

    useActivityFeedStore.getState().replaceEvents([event], null);

    render(
      <ProjectActivityFeed
        projectId="proj-1"
        isProjectsLoading={false}
        onEventDismiss={() => undefined}
      />
    );

    const card = screen.getByRole('button', { name: event.headline });

    fireEvent.pointerDown(card, {
      pointerId: 1,
      pointerType: 'touch',
      isPrimary: true,
      clientX: 10,
      clientY: 10,
    });

    fireEvent.pointerUp(card, {
      pointerId: 1,
      pointerType: 'touch',
      isPrimary: true,
      clientX: 12,
      clientY: 13,
    });

    expect(trackAnalyticsEvent).toHaveBeenCalledWith(
      'activity_feed.view_item',
      expect.objectContaining({ eventId: event.id })
    );
    expect(navigateMock).toHaveBeenCalledWith(event.cta?.href);
  });

  it('navigates when tapped in environments without PointerEvent support', () => {
    const originalPointerEvent = window.PointerEvent;
    // Simulate Safari/iOS 12 style browsers without PointerEvent
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-expect-error - deleting for test scenario
    delete window.PointerEvent;

    const event = stubEvent({ id: 'touch-only' });

    useActivityFeedStore.getState().replaceEvents([event], null);

    render(
      <ProjectActivityFeed
        projectId="proj-1"
        isProjectsLoading={false}
        onEventDismiss={() => undefined}
      />
    );

    const card = screen.getByRole('button', { name: event.headline });

    fireEvent.touchStart(card, {
      touches: [
        {
          clientX: 40,
          clientY: 50,
        },
      ],
    });

    fireEvent.touchEnd(card, {
      changedTouches: [
        {
          clientX: 42,
          clientY: 52,
        },
      ],
    });

    expect(trackAnalyticsEvent).toHaveBeenCalledWith(
      'activity_feed.view_item',
      expect.objectContaining({ eventId: event.id })
    );
    expect(navigateMock).toHaveBeenCalledWith(event.cta?.href);

    if (originalPointerEvent) {
      Object.defineProperty(window, 'PointerEvent', {
        configurable: true,
        writable: true,
        value: originalPointerEvent,
      });
    } else {
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
      // @ts-expect-error - ensure clean state
      delete window.PointerEvent;
    }
  });

  it('does not navigate when dismissing an urgent event', async () => {
    const user = userEvent.setup();
    const event = stubEvent();
    const markAsHandled = vi.fn();

    mockUseActivityFeed.mockReturnValue({
      ...defaultFeedApi,
      markAsHandled,
    });

    useActivityFeedStore.getState().replaceEvents([event], null);

    render(
      <ProjectActivityFeed
        projectId="proj-1"
        isProjectsLoading={false}
        onEventDismiss={() => undefined}
      />
    );

    const dismissButton = screen.getByRole('button', {
      name: /dismiss from high priority/i,
    });
    await user.click(dismissButton);

    expect(markAsHandled).toHaveBeenCalledWith(event.id);
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it('shows reconnect banner when websocket disconnects', () => {
    const event = stubEvent();
    const store = useActivityFeedStore.getState();
    store.replaceEvents([event], null);
    useActivityFeedStore.setState((state) => ({
      ...state,
      connectionState: 'reconnecting',
      connectionAttempts: 3,
      hasConnectedOnce: true,
    }));

    render(<ProjectActivityFeed projectId="proj-1" isProjectsLoading={false} />);

    expect(
      screen.getByText(/reconnecting to live updates/i)
    ).toBeInTheDocument();
  });

  it('invokes setScope when filter changes', async () => {
    const user = userEvent.setup();
    const setFilterMock = vi.fn();
    mockUseActivityFeed.mockReturnValue({
      loadMore: vi.fn(),
      setFilter: setFilterMock,
      markAsHandled: vi.fn(),
      hasMore: false,
      isFetchingNextPage: false,
    });

    render(<ProjectActivityFeed projectId="proj-1" isProjectsLoading={false} />);

    await user.click(screen.getByRole('tab', { name: /needs review/i }));

    expect(setFilterMock).toHaveBeenCalledWith('need_review');
  });

  it('surfaces in-review events under the Needs Review tab even without actionRequired flag', () => {
    const reviewEvent = stubEvent({
      actionRequired: false,
      urgencyScore: 55,
      summary: 'Status: inreview',
      headline: 'Task pending review',
    });

    useActivityFeedStore.getState().replaceEvents([reviewEvent], null);

    render(<ProjectActivityFeed projectId="proj-1" isProjectsLoading={false} />);

    expect(screen.getByText(/task pending review/i)).toBeInTheDocument();
  });
});
