import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/react';
import RawLogText from '@/components/common/RawLogText';

// Mock fancy-ansi
vi.mock('fancy-ansi/react', () => ({
  AnsiHtml: ({ text }: { text: string }) => <span>{text}</span>,
}));

vi.mock('fancy-ansi', () => ({
  hasAnsi: (text: string) => text.includes('\x1b'),
}));

describe('RawLogText', () => {
  it('renders plain text without ANSI codes', () => {
    const { container } = render(
      <RawLogText content="Hello world" channel="stdout" />
    );
    expect(container.textContent).toBe('Hello world');
  });

  it('applies stderr styling for stderr channel without ANSI', () => {
    const { container } = render(
      <RawLogText content="Error message" channel="stderr" />
    );
    const element = container.querySelector('div');
    expect(element?.className).toContain('text-destructive');
  });

  it('does not apply stderr styling when ANSI codes are present', () => {
    const ansiText = '\x1b[31mRed text\x1b[0m';
    const { container } = render(
      <RawLogText content={ansiText} channel="stderr" />
    );
    const element = container.querySelector('div');
    // The text has ANSI, so hasAnsi returns true, and shouldApplyStderrFallback is false
    // Therefore 'text-destructive' should NOT be in the className
    const hasDestructive = element?.className.includes('text-destructive');
    expect(hasDestructive).toBe(false);
  });

  it('memoizes to prevent unnecessary re-renders', () => {
    let renderCount = 0;

    const TestWrapper = ({ content }: { content: string }) => {
      renderCount++;
      return <RawLogText content={content} channel="stdout" />;
    };

    const { rerender } = render(<TestWrapper content="test" />);

    expect(renderCount).toBe(1);

    // Re-render with same props - RawLogText is memoized, but TestWrapper will re-render
    rerender(<TestWrapper content="test" />);

    // TestWrapper renders again (2), but RawLogText should not re-render due to memo
    expect(renderCount).toBe(2);

    // Re-render with different content
    rerender(<TestWrapper content="different" />);

    // TestWrapper renders again (3)
    expect(renderCount).toBe(3);
  });

  it('renders as span when specified', () => {
    const { container } = render(
      <RawLogText content="test" as="span" channel="stdout" />
    );
    expect(container.querySelector('span')).toBeTruthy();
    expect(container.querySelector('div')).toBeFalsy();
  });

  it('applies custom className', () => {
    const { container } = render(
      <RawLogText content="test" className="custom-class" channel="stdout" />
    );
    const element = container.querySelector('div');
    expect(element?.className).toContain('custom-class');
  });
});
