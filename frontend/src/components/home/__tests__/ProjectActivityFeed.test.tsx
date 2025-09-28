import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
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

vi.mock('@/hooks/useActivityFeed', () => ({
  useActivityFeed: (options: unknown) =>
    mockUseActivityFeed(options) ?? defaultFeedApi,
}));

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
  id: 'evt-1',
  headline: 'New deployment ready',
  summary: 'Deployment completed successfully',
  cta: { label: 'View', href: '/deployments/1' },
  urgencyScore: 82,
  actionRequired: true,
  createdAt: new Date('2025-09-20T12:00:00Z'),
  ...overrides,
});

beforeEach(() => {
  resetStore();
  mockUseActivityFeed.mockReturnValue({
    loadMore: vi.fn(),
    setScope: vi.fn(),
    markAsHandled: vi.fn(),
    hasMore: false,
    isFetchingNextPage: false,
  });
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

    render(<ProjectActivityFeed projectId="proj-1" isProjectsLoading={false} />);

    expect(screen.getByText(/you're all caught up/i)).toBeInTheDocument();
  });

  it('renders event with CTA and tracks analytics on click', async () => {
    const user = userEvent.setup();
    const event = stubEvent();

    useActivityFeedStore.getState().replaceEvents([event], null);

    render(<ProjectActivityFeed projectId="proj-1" isProjectsLoading={false} />);

    const cta = screen.getByRole('link', { name: /view/i });
    await user.click(cta);

    expect(trackAnalyticsEvent).toHaveBeenCalledWith(
      'activity_feed.view_item',
      expect.objectContaining({ eventId: event.id })
    );
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

    await user.click(screen.getByRole('tab', { name: /need review/i }));

    expect(setFilterMock).toHaveBeenCalledWith('need_review');
  });
});
