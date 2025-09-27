import { useMemo, useState } from 'react';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertCircle, Loader2, Trash2 } from 'lucide-react';
import { useProject } from '@/contexts/project-context';
import type { ProjectRepository } from 'shared/types';

export interface DeleteRepositoryDialogProps {
  repository: ProjectRepository;
}

export type DeleteRepositoryDialogResult = 'deleted' | 'canceled';

export const DeleteRepositoryDialog = NiceModal.create<DeleteRepositoryDialogProps>(
  ({ repository }) => {
    const modal = useModal();
    const { repositories, deleteRepository, isRepositoryMutating } = useProject();
    const [error, setError] = useState<string | null>(null);
    const [submitting, setSubmitting] = useState(false);

    const isOnlyRepository = useMemo(
      () => repositories.filter((repo) => repo.id !== repository.id).length === 0,
      [repositories, repository.id]
    );

    const handleClose = (result: DeleteRepositoryDialogResult) => {
      modal.resolve(result);
      modal.hide();
    };

    const handleOpenChange = (open: boolean) => {
      if (!open) {
        handleClose('canceled');
      }
    };

    const handleDelete = async () => {
      if (isOnlyRepository) {
        return;
      }

      setError(null);
      setSubmitting(true);

      try {
        await deleteRepository(repository.id);
        handleClose('deleted');
      } catch (err) {
        setError(
          err instanceof Error ? err.message : 'Failed to delete repository'
        );
      } finally {
        setSubmitting(false);
      }
    };

    return (
      <Dialog open={modal.visible} onOpenChange={handleOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove repository</DialogTitle>
            <DialogDescription>
              Confirm removing this repository from the project.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4">
            <div className="rounded-md border border-border bg-muted/40 p-4 text-sm">
              <p className="font-medium">{repository.name}</p>
              <p className="mt-1 break-all text-xs text-muted-foreground">
                {repository.git_repo_path}
              </p>
              {repository.root_path ? (
                <p className="mt-1 break-all text-xs text-muted-foreground">
                  root: {repository.root_path}
                </p>
              ) : null}
            </div>

            {error ? (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            ) : null}

            {isOnlyRepository ? (
              <Alert>
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>
                  Each project must keep a primary repository. Promote another
                  repository before removing this one.
                </AlertDescription>
              </Alert>
            ) : null}
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => handleClose('canceled')}
            >
              Cancel
            </Button>
            <Button
              type="button"
              variant="destructive"
              onClick={handleDelete}
              disabled={submitting || isRepositoryMutating || isOnlyRepository}
            >
              {(submitting || isRepositoryMutating) && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              <Trash2 className="mr-2 h-4 w-4" /> Remove
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
);

export default DeleteRepositoryDialog;
