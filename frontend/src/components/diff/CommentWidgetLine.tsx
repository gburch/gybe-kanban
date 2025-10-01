import { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import { Button } from '@/components/ui/button';
import { FileSearchTextarea } from '@/components/ui/file-search-textarea';
import { useReview, type ReviewDraft } from '@/contexts/ReviewProvider';
import { Scope, useKeyExit } from '@/keyboard';
import { useHotkeysContext } from 'react-hotkeys-hook';

interface CommentWidgetLineProps {
  draft: ReviewDraft;
  widgetKey: string;
  onSave: () => void;
  onCancel: () => void;
  projectId?: string;
  repositoryId?: string | null;
}

export function CommentWidgetLine({
  draft,
  widgetKey,
  onSave,
  onCancel,
  projectId,
  repositoryId,
}: CommentWidgetLineProps) {
  const { setDraft, addComment } = useReview();
  const [value, setValue] = useState(draft.text);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const { enableScope, disableScope } = useHotkeysContext();

  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  useEffect(() => {
    enableScope(Scope.EDIT_COMMENT);
    return () => {
      disableScope(Scope.EDIT_COMMENT);
    };
  }, [enableScope, disableScope]);

  const handleCancel = useCallback(() => {
    setDraft(widgetKey, null);
    onCancel();
  }, [setDraft, widgetKey, onCancel]);

  const exitOptions = useMemo(
    () => ({
      scope: Scope.EDIT_COMMENT,
    }),
    []
  );

  useKeyExit(handleCancel, exitOptions);

  const handleSave = () => {
    if (value.trim()) {
      addComment({
        filePath: draft.filePath,
        side: draft.side,
        lineNumber: draft.lineNumber,
        text: value.trim(),
        codeLine: draft.codeLine,
      });
    }
    setDraft(widgetKey, null);
    onSave();
  };

  return (
    <div className="p-4 border-y">
      <FileSearchTextarea
        value={value}
        onChange={setValue}
        placeholder="Add a comment... (type @ to search files)"
        rows={3}
        maxRows={10}
        className="w-full bg-primary text-primary-foreground text-base font-mono resize-none min-h-[60px] focus:outline-none focus:ring-1 focus:ring-primary"
        projectId={projectId}
        repositoryId={repositoryId ?? undefined}
        onCommandEnter={handleSave}
      />
      <div className="mt-2 flex gap-2">
        <Button size="xs" onClick={handleSave} disabled={!value.trim()}>
          Add review comment
        </Button>
        <Button
          size="xs"
          variant="ghost"
          onClick={handleCancel}
          className="text-secondary-foreground"
        >
          Cancel
        </Button>
      </div>
    </div>
  );
}
