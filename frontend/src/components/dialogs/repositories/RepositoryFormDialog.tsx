import { useState, useMemo } from 'react';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertCircle, FolderSearch, Loader2 } from 'lucide-react';
import { showFolderPicker } from '@/lib/modals';
import { useProject } from '@/contexts/project-context';
import type {
  ProjectRepository,
  CreateProjectRepository,
  UpdateProjectRepository,
} from 'shared/types';

export interface RepositoryFormDialogProps {
  repository?: ProjectRepository;
  initialGitRepoPath?: string;
}

export type RepositoryFormDialogResult = 'saved' | 'canceled';

export const RepositoryFormDialog = NiceModal.create<RepositoryFormDialogProps>(
  ({ repository, initialGitRepoPath }) => {
    const modal = useModal();
    const {
      repositories,
      createRepository,
      updateRepository,
      isRepositoryMutating,
    } = useProject();

    const isEditing = !!repository;
    const isFirstRepository = !isEditing && repositories.length === 0;

    const [name, setName] = useState(
      repository?.name ?? (isFirstRepository ? 'Primary' : '')
    );
    const [gitRepoPath, setGitRepoPath] = useState(
      repository?.git_repo_path ?? initialGitRepoPath ?? ''
    );
    const [rootPath, setRootPath] = useState(repository?.root_path ?? '');
    const [isPrimary, setIsPrimary] = useState(
      repository?.is_primary ?? isFirstRepository
    );
    const [error, setError] = useState<string | null>(null);
    const [submitting, setSubmitting] = useState(false);

    const primaryToggleDisabled = useMemo(() => {
      if (!isEditing) {
        return repositories.length === 0 && !isPrimary;
      }
      if (!repository?.is_primary) return false;
      return repositories.filter((repo) => repo.id !== repository.id).length === 0;
    }, [isEditing, repositories, repository, isPrimary]);

    const handleClose = (result: RepositoryFormDialogResult) => {
      modal.resolve(result);
      modal.hide();
    };

    const handleOpenChange = (open: boolean) => {
      if (!open) {
        handleClose('canceled');
      }
    };

    const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      setError(null);

      const trimmedName = name.trim();
      const trimmedPath = gitRepoPath.trim();
      const trimmedRoot = rootPath.trim();

      if (!trimmedName) {
        setError('Repository name is required.');
        return;
      }

      if (!trimmedPath) {
        setError('Repository path is required.');
        return;
      }

      setSubmitting(true);

      try {
        if (isEditing && repository) {
          const payload: UpdateProjectRepository = {
            name:
              trimmedName !== repository.name ? trimmedName : null,
            git_repo_path:
              trimmedPath !== repository.git_repo_path ? trimmedPath : null,
            root_path:
              trimmedRoot !== repository.root_path ? trimmedRoot : null,
            is_primary:
              isPrimary !== repository.is_primary ? isPrimary : null,
          };

          await updateRepository(repository.id, payload);
        } else {
          const payload: CreateProjectRepository = {
            name: trimmedName,
            git_repo_path: trimmedPath,
            root_path: trimmedRoot ? trimmedRoot : null,
            is_primary: isPrimary,
          };

          await createRepository(payload);
        }

        handleClose('saved');
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to save repository');
      } finally {
        setSubmitting(false);
      }
    };

    const handleBrowse = async () => {
      setError(null);
      try {
        const selected = await showFolderPicker({
          title: 'Select repository folder',
          value: gitRepoPath || undefined,
        });
        if (selected) {
          setGitRepoPath(selected);
        }
      } catch (err) {
        setError('Unable to open folder picker.');
        console.error(err);
      }
    };

    return (
      <Dialog open={modal.visible} onOpenChange={handleOpenChange}>
        <DialogContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <DialogHeader>
              <DialogTitle>{
                isEditing ? 'Edit repository' : 'Add repository'
              }</DialogTitle>
              <DialogDescription>
                Configure repository details for this project.
              </DialogDescription>
            </DialogHeader>

            {error ? (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            ) : null}

            <div className="space-y-2">
              <Label htmlFor="repository-name">Repository name</Label>
              <Input
                id="repository-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="Docs, Mobile, API"
                autoFocus
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="repository-path">Git repository path</Label>
              <div className="flex gap-2">
                <Input
                  id="repository-path"
                  value={gitRepoPath}
                  onChange={(event) => setGitRepoPath(event.target.value)}
                  placeholder="/Users/me/workspace/project"
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleBrowse}
                  className="shrink-0"
                >
                  <FolderSearch className="mr-2 h-4 w-4" /> Browse
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                Provide an absolute path to a local Git repository.
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="repository-root">Repository root (optional)</Label>
              <Input
                id="repository-root"
                value={rootPath}
                onChange={(event) => setRootPath(event.target.value)}
                placeholder="apps/api"
              />
              <p className="text-xs text-muted-foreground">
                Limit the agent to a folder inside the repository. Leave blank to
                index the entire repository.
              </p>
            </div>

            <div className="flex items-center justify-between rounded-md border border-border bg-muted/40 px-3 py-2">
              <div className="space-y-1">
                <p className="text-sm font-medium">Set as primary</p>
                <p className="text-xs text-muted-foreground">
                  The primary repository is used for pull requests and diff views.
                </p>
              </div>
              <Switch
                checked={isPrimary}
                disabled={primaryToggleDisabled}
                onCheckedChange={setIsPrimary}
                aria-label="Toggle primary repository"
              />
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
                type="submit"
                disabled={submitting || isRepositoryMutating}
              >
                {(submitting || isRepositoryMutating) && (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                )}
                Save repository
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    );
  }
);

export default RepositoryFormDialog;
