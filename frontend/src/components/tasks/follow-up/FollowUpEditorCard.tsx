import { Loader2 } from 'lucide-react';
import { FileSearchTextarea } from '@/components/ui/file-search-textarea';
import { cn } from '@/lib/utils';
import { useProject } from '@/contexts/project-context';

type Props = {
  placeholder: string;
  value: string;
  onChange: (v: string) => void;
  onKeyDown?: (e: React.KeyboardEvent<Element>) => void;
  disabled: boolean;
  // Loading overlay
  showLoadingOverlay: boolean;
  onCommandEnter?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onCommandShiftEnter?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  textareaClassName?: string;
};

export function FollowUpEditorCard({
  placeholder,
  value,
  onChange,
  onKeyDown,
  disabled,
  showLoadingOverlay,
  onCommandEnter,
  onCommandShiftEnter,
  textareaClassName,
}: Props) {
  const { projectId, selectedRepositoryId } = useProject();
  return (
    <div className="relative">
      <FileSearchTextarea
        placeholder={placeholder}
        value={value}
        onChange={onChange}
        onKeyDown={onKeyDown}
        className={cn('flex-1 min-h-[40px] resize-none', textareaClassName)}
        disabled={disabled}
        projectId={projectId}
        repositoryId={selectedRepositoryId ?? undefined}
        rows={1}
        maxRows={6}
        onCommandEnter={onCommandEnter}
        onCommandShiftEnter={onCommandShiftEnter}
      />
      {showLoadingOverlay && (
        <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center bg-background/60">
          <Loader2 className="h-4 w-4 animate-spin" />
        </div>
      )}
    </div>
  );
}
