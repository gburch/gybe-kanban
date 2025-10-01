import {
  DataWithScrollModifier,
  ScrollModifier,
  VirtuosoMessageList,
  VirtuosoMessageListLicense,
  VirtuosoMessageListMethods,
  VirtuosoMessageListProps,
} from '@virtuoso.dev/message-list';
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso';
import { useEffect, useMemo, useRef, useState } from 'react';

import DisplayConversationEntry from '../NormalizedConversation/DisplayConversationEntry';
import { useEntries } from '@/contexts/EntriesContext';
import {
  AddEntryType,
  PatchTypeWithKey,
  useConversationHistory,
} from '@/hooks/useConversationHistory';
import { Loader2 } from 'lucide-react';
import { TaskAttempt } from 'shared/types';

interface VirtualizedListProps {
  attempt: TaskAttempt;
}

interface MessageListContext {
  attempt: TaskAttempt;
}

const INITIAL_TOP_ITEM = { index: 'LAST' as const, align: 'end' as const };

const InitialDataScrollModifier: ScrollModifier = {
  type: 'item-location',
  location: INITIAL_TOP_ITEM,
  purgeItemSizes: true,
};

const AutoScrollToBottom: ScrollModifier = {
  type: 'auto-scroll-to-bottom',
  autoScroll: 'smooth',
};

const ItemContent: VirtuosoMessageListProps<
  PatchTypeWithKey,
  MessageListContext
>['ItemContent'] = ({ data, context }) => {
  const attempt = context?.attempt;

  if (data.type === 'STDOUT') {
    return <p>{data.content}</p>;
  }
  if (data.type === 'STDERR') {
    return <p>{data.content}</p>;
  }
  if (data.type === 'NORMALIZED_ENTRY' && attempt) {
    return (
      <DisplayConversationEntry
        expansionKey={data.patchKey}
        entry={data.content}
        executionProcessId={data.executionProcessId}
        taskAttempt={attempt}
      />
    );
  }

  return null;
};

const computeItemKey: VirtuosoMessageListProps<
  PatchTypeWithKey,
  MessageListContext
>['computeItemKey'] = ({ data }) => `l-${data.patchKey}`;

const VirtualizedList = ({ attempt }: VirtualizedListProps) => {
  const [channelData, setChannelData] =
    useState<DataWithScrollModifier<PatchTypeWithKey> | null>(null);
  const [loading, setLoading] = useState(true);
  const { setEntries, reset } = useEntries();
  const licenseKey = import.meta.env.VITE_PUBLIC_REACT_VIRTUOSO_LICENSE_KEY;
  const fallbackVirtuosoRef = useRef<VirtuosoHandle | null>(null);

  useEffect(() => {
    setLoading(true);
    setChannelData(null);
    reset();
  }, [attempt.id, reset]);

  const onEntriesUpdated = (
    newEntries: PatchTypeWithKey[],
    addType: AddEntryType,
    newLoading: boolean
  ) => {
    let scrollModifier: ScrollModifier = InitialDataScrollModifier;

    if (addType === 'running' && !loading) {
      scrollModifier = AutoScrollToBottom;
    }

    setChannelData({ data: newEntries, scrollModifier });
    setEntries(newEntries);

    if (loading) {
      setLoading(newLoading);
    }
  };

  useConversationHistory({ attempt, onEntriesUpdated });

  const messageListRef = useRef<VirtuosoMessageListMethods | null>(null);
  const messageListContext = useMemo(() => ({ attempt }), [attempt]);
  const fallbackData = channelData?.data ?? [];

  const useLicensedList = useMemo(() => {
    if (licenseKey) {
      return true;
    }

    if (typeof window === 'undefined') {
      return true;
    }

    const hostname = window.location.hostname;
    return /^(localhost|127\.0\.0\.1|0\.0\.0\.0|.+\.local)$/i.test(hostname);
  }, [licenseKey]);

  useEffect(() => {
    if (useLicensedList || !channelData || !fallbackVirtuosoRef.current) {
      return;
    }

    if (channelData.scrollModifier === AutoScrollToBottom) {
      fallbackVirtuosoRef.current.scrollToIndex({
        index: Math.max(fallbackData.length - 1, 0),
        behavior: 'smooth',
        align: 'end',
      });
    }

    if (channelData.scrollModifier === InitialDataScrollModifier) {
      fallbackVirtuosoRef.current.scrollToIndex({
        index: Math.max(fallbackData.length - 1, 0),
        behavior: 'auto',
        align: INITIAL_TOP_ITEM.align,
      });
    }
  }, [channelData, fallbackData.length, useLicensedList]);

  return (
    <>
      {useLicensedList ? (
        <VirtuosoMessageListLicense licenseKey={licenseKey}>
          <VirtuosoMessageList<PatchTypeWithKey, MessageListContext>
            ref={messageListRef}
            className="flex-1"
            data={channelData}
            initialLocation={INITIAL_TOP_ITEM}
            context={messageListContext}
            computeItemKey={computeItemKey}
            ItemContent={ItemContent}
            Header={() => <div className="h-2"></div>}
            Footer={() => <div className="h-2"></div>}
          />
        </VirtuosoMessageListLicense>
      ) : (
        <Virtuoso<PatchTypeWithKey>
          ref={fallbackVirtuosoRef}
          className="flex-1"
          data={fallbackData}
          followOutput="smooth"
          itemContent={(index, item) => (
            <ItemContent
              index={index}
              data={item}
              prevData={fallbackData[index - 1] ?? null}
              nextData={fallbackData[index + 1] ?? null}
              context={messageListContext}
            />
          )}
        />
      )}
      {loading && (
        <div className="float-left top-0 left-0 w-full h-full bg-primary flex flex-col gap-2 justify-center items-center">
          <Loader2 className="h-8 w-8 animate-spin" />
          <p>Loading History</p>
        </div>
      )}
    </>
  );
};

export default VirtualizedList;
