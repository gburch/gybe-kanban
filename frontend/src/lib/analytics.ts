export type AnalyticsPayload = Record<string, unknown>;

const EVENT_NAME = 'analytics:event';

export const trackAnalyticsEvent = (
  name: string,
  payload: AnalyticsPayload = {}
) => {
  if (typeof window === 'undefined') {
    return;
  }

  try {
    const detail = { name, payload, timestamp: Date.now() };
    window.dispatchEvent(new CustomEvent(EVENT_NAME, { detail }));

    if (typeof navigator !== 'undefined' && navigator.sendBeacon) {
      const body = JSON.stringify(detail);
      navigator.sendBeacon('/api/analytics/event', body);
    }
  } catch (error) {
    if (import.meta.env.MODE !== 'production') {
      console.debug('[analytics] failed to emit event', name, error);
    }
  }
};
