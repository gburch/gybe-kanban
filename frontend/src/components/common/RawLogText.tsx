import { memo, useMemo, useState } from 'react';
import { AnsiHtml } from 'fancy-ansi/react';
import { hasAnsi } from 'fancy-ansi';
import { clsx } from 'clsx';

interface RawLogTextProps {
  content: string;
  channel?: 'stdout' | 'stderr';
  as?: 'div' | 'span';
  className?: string;
}

// Maximum content length before truncation (10KB)
const MAX_CONTENT_LENGTH = 10000;

const RawLogText = memo(
  ({
    content,
    channel = 'stdout',
    as: Component = 'div',
    className,
  }: RawLogTextProps) => {
    const [isExpanded, setIsExpanded] = useState(false);

    // Memoize ANSI detection to avoid re-parsing on every render
    const hasAnsiCodes = useMemo(() => hasAnsi(content), [content]);
    const shouldApplyStderrFallback = channel === 'stderr' && !hasAnsiCodes;

    // Truncate very long content to prevent UI hang
    const isTruncated = content.length > MAX_CONTENT_LENGTH;
    const displayContent = isTruncated && !isExpanded
      ? content.substring(0, MAX_CONTENT_LENGTH)
      : content;

    return (
      <Component
        className={clsx(
          'font-mono text-xs break-all whitespace-pre-wrap',
          shouldApplyStderrFallback && 'text-destructive',
          className
        )}
      >
        <AnsiHtml text={displayContent} />
        {isTruncated && (
          <button
            onClick={() => setIsExpanded(!isExpanded)}
            className="ml-2 text-blue-500 hover:text-blue-700 underline text-xs"
          >
            {isExpanded
              ? '▲ Show less'
              : `▼ Show more (${Math.round((content.length - MAX_CONTENT_LENGTH) / 1024)}KB truncated)`
            }
          </button>
        )}
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
