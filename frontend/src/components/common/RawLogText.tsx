import { memo, useMemo } from 'react';
import { AnsiHtml } from 'fancy-ansi/react';
import { hasAnsi } from 'fancy-ansi';
import { clsx } from 'clsx';

interface RawLogTextProps {
  content: string;
  channel?: 'stdout' | 'stderr';
  as?: 'div' | 'span';
  className?: string;
}

const RawLogText = memo(
  ({
    content,
    channel = 'stdout',
    as: Component = 'div',
    className,
  }: RawLogTextProps) => {
    // Memoize ANSI detection to avoid re-parsing on every render
    const hasAnsiCodes = useMemo(() => hasAnsi(content), [content]);
    const shouldApplyStderrFallback = channel === 'stderr' && !hasAnsiCodes;

    return (
      <Component
        className={clsx(
          'font-mono text-xs break-all whitespace-pre-wrap',
          shouldApplyStderrFallback && 'text-destructive',
          className
        )}
      >
        <AnsiHtml text={content} />
      </Component>
    );
  },
  // Custom comparison to prevent unnecessary re-renders
  (prevProps, nextProps) =>
    prevProps.content === nextProps.content &&
    prevProps.channel === nextProps.channel &&
    prevProps.className === nextProps.className &&
    prevProps.as === nextProps.as
);

RawLogText.displayName = 'RawLogText';

export default RawLogText;
