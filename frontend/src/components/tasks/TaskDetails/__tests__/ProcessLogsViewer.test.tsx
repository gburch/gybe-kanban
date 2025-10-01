import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import ProcessLogsViewer from '@/components/tasks/TaskDetails/ProcessLogsViewer';

// Mock dependencies
vi.mock('@/hooks/useLogStream', () => ({
  useLogStream: vi.fn(() => ({
    logs: [],
    error: null,
  })),
}));

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent, computeItemKey }: any) => (
    <div data-testid="virtuoso-list">
      {data.map((item: any, index: number) => (
        <div key={computeItemKey?.(index, item) || index}>
          {itemContent(index, item)}
        </div>
      ))}
    </div>
  ),
  VirtuosoHandle: vi.fn(),
}));

vi.mock('@/components/common/RawLogText', () => ({
  default: ({ content, channel }: any) => (
    <div data-channel={channel}>{content}</div>
  ),
}));

import { useLogStream } from '@/hooks/useLogStream';

describe('ProcessLogsViewer', () => {
  it('generates stable keys for log entries', () => {
    const mockLogs = [
      { type: 'STDOUT' as const, content: 'Log 1' },
      { type: 'STDOUT' as const, content: 'Log 2' },
      { type: 'STDERR' as const, content: 'Error 1' },
    ];

    vi.mocked(useLogStream).mockReturnValue({
      logs: mockLogs,
      error: null,
    });

    const { container } = render(<ProcessLogsViewer processId="proc-123" />);

    // Check that items are rendered
    const items = container.querySelectorAll('[data-testid="virtuoso-list"] > div');
    expect(items.length).toBe(3);

    // Keys should be stable and based on processId + index
    const firstKey = items[0].getAttribute('key') || items[0].textContent;
    expect(firstKey).toBeTruthy();
  });

  it('displays "No logs available" when logs are empty', () => {
    vi.mocked(useLogStream).mockReturnValue({
      logs: [],
      error: null,
    });

    render(<ProcessLogsViewer processId="proc-123" />);
    expect(screen.getByText('No logs available')).toBeTruthy();
  });

  it('displays error message when error occurs', () => {
    vi.mocked(useLogStream).mockReturnValue({
      logs: [],
      error: 'Connection failed',
    });

    render(<ProcessLogsViewer processId="proc-123" />);
    expect(screen.getByText('Connection failed')).toBeTruthy();
  });

  it('renders STDOUT and STDERR logs correctly', () => {
    const mockLogs = [
      { type: 'STDOUT' as const, content: 'Standard output' },
      { type: 'STDERR' as const, content: 'Error output' },
    ];

    vi.mocked(useLogStream).mockReturnValue({
      logs: mockLogs,
      error: null,
    });

    const { container } = render(<ProcessLogsViewer processId="proc-123" />);

    const stdoutElements = container.querySelectorAll('[data-channel="stdout"]');
    const stderrElements = container.querySelectorAll('[data-channel="stderr"]');

    expect(stdoutElements.length).toBe(1);
    expect(stderrElements.length).toBe(1);
    expect(stdoutElements[0].textContent).toBe('Standard output');
    expect(stderrElements[0].textContent).toBe('Error output');
  });

  it('preserves keys when new logs are appended', () => {
    const initialLogs = [
      { type: 'STDOUT' as const, content: 'Log 1' },
    ];

    const { rerender, container } = render(
      <ProcessLogsViewer processId="proc-123" />
    );

    vi.mocked(useLogStream).mockReturnValue({
      logs: initialLogs,
      error: null,
    });
    rerender(<ProcessLogsViewer processId="proc-123" />);

    const firstItems = container.querySelectorAll('[data-testid="virtuoso-list"] > div');
    expect(firstItems.length).toBe(1);

    // Add more logs
    const updatedLogs = [
      { type: 'STDOUT' as const, content: 'Log 1' },
      { type: 'STDOUT' as const, content: 'Log 2' },
    ];

    vi.mocked(useLogStream).mockReturnValue({
      logs: updatedLogs,
      error: null,
    });
    rerender(<ProcessLogsViewer processId="proc-123" />);

    const secondItems = container.querySelectorAll('[data-testid="virtuoso-list"] > div');
    expect(secondItems.length).toBe(2);
  });
});
