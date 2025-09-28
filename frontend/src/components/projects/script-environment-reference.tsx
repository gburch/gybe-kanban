import { useEffect, useMemo, useState } from 'react';
import type { ProjectRepository } from 'shared/types';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Copy, Check, XCircle } from 'lucide-react';
import { getRepositoryEnvSummary } from '@/utils/repo-env';

interface ScriptEnvironmentReferenceProps {
  repositories: ProjectRepository[];
}

type Option =
  | { value: 'primary'; label: string }
  | {
      value: string;
      label: string;
      repository: ProjectRepository;
      prefix: string;
    };

type TokenEntry = {
  label: string;
  description: string;
  value: string;
};

export function ScriptEnvironmentReference({
  repositories,
}: ScriptEnvironmentReferenceProps) {
  const primaryRepo = useMemo(
    () => repositories.find((repo) => repo.is_primary) ?? repositories[0],
    [repositories]
  );

  const options = useMemo<Option[]>(() => {
    const repoOptions = repositories.map<Option>((repo) => {
      const { prefix } = getRepositoryEnvSummary(repo);
      const labelParts = [repo.name];
      if (repo.is_primary) {
        labelParts.push('(Primary)');
      }
      labelParts.push(`prefix ${prefix}`);
      return {
        value: repo.id,
        label: labelParts.join(' '),
        repository: repo,
        prefix,
      };
    });
    if (!repositories.length) {
      return [{ value: 'primary', label: 'Primary repository context' }];
    }
    return [
      {
        value: 'primary',
        label: 'Primary repository context',
      },
      ...repoOptions,
    ];
  }, [repositories]);

  const [selectedOption, setSelectedOption] = useState<Option>(() => options[0]);
  const [copiedToken, setCopiedToken] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);

  useEffect(() => {
    if (!options.length) {
      return;
    }
    if (selectedOption && options.some((option) => option.value === selectedOption.value)) {
      return;
    }
    const next = options.find((option) => option.value === primaryRepo?.id) ?? options[0];
    setSelectedOption(next);
  }, [options, primaryRepo, selectedOption]);

  useEffect(() => {
    if (!copiedToken && !copyError) {
      return;
    }
    const timer = window.setTimeout(() => {
      setCopiedToken(null);
      setCopyError(null);
    }, 2000);
    return () => window.clearTimeout(timer);
  }, [copiedToken, copyError]);

  if (!options.length) {
    return null;
  }

  const handleCopy = async (entry: TokenEntry) => {
    try {
      const canUseNavigator =
        typeof navigator !== 'undefined' && navigator.clipboard && navigator.clipboard.writeText;
      if (canUseNavigator) {
        await navigator.clipboard.writeText(entry.value);
      } else if (typeof document !== 'undefined') {
        const textarea = document.createElement('textarea');
        textarea.value = entry.value;
        textarea.style.position = 'fixed';
        textarea.style.left = '-9999px';
        document.body.appendChild(textarea);
        textarea.focus();
        textarea.select();
        document.execCommand('copy');
        document.body.removeChild(textarea);
      } else {
        throw new Error('Clipboard API is not available');
      }
      setCopiedToken(entry.value);
      setCopyError(null);
    } catch (error) {
      console.error('Failed to copy token', error);
      setCopiedToken(entry.value);
      setCopyError('Copy failed');
    }
  };

  const renderTokens = (tokens: TokenEntry[]) => (
    <div className="space-y-2">
      {tokens.map((token) => {
        const isCopied = copiedToken === token.value && !copyError;
        const isError = copyError && copiedToken === token.value;
        return (
          <div
            key={token.value}
            className="flex items-center justify-between gap-2 rounded-md border border-border bg-background px-3 py-2"
          >
            <div className="flex-1">
              <code className="text-sm font-mono text-foreground">{token.label}</code>
              <p className="text-xs text-muted-foreground">{token.description}</p>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="flex items-center gap-1"
              onClick={() => handleCopy(token)}
            >
              {isCopied ? (
                <>
                  <Check className="h-4 w-4" />
                  Copied
                </>
              ) : isError ? (
                <>
                  <XCircle className="h-4 w-4" />
                  Retry
                </>
              ) : (
                <>
                  <Copy className="h-4 w-4" />
                  Copy
                </>
              )}
            </Button>
          </div>
        );
      })}
    </div>
  );

  const primaryTokens: TokenEntry[] = [
    {
      label: 'VIBE_PRIMARY_REPO_PATH',
      description: 'Absolute path to the primary worktree.',
      value: 'VIBE_PRIMARY_REPO_PATH',
    },
    {
      label: 'VIBE_PRIMARY_REPO_ROOT',
      description: 'Root override for the primary repository, if any.',
      value: 'VIBE_PRIMARY_REPO_ROOT',
    },
    {
      label: 'VIBE_PRIMARY_REPO_PREFIX',
      description: 'Environment prefix for the primary repository.',
      value: 'VIBE_PRIMARY_REPO_PREFIX',
    },
    {
      label: 'VIBE_REPOSITORIES',
      description: 'Comma-separated list of repository prefixes.',
      value: 'VIBE_REPOSITORIES',
    },
  ];

  const renderPrimaryContext = () => {
    return renderTokens(primaryTokens);
  };

  const renderRepositoryContext = (option: Extract<Option, { repository: ProjectRepository }>) => {
    const repo = option.repository;
    const summary = getRepositoryEnvSummary(repo);
    const repoTokens: TokenEntry[] = [
      {
        label: summary.pathVar,
        description: 'Worktree path for this repository.',
        value: summary.pathVar,
      },
      {
        label: summary.rootVar,
        description: 'Root override (empty when exposing the whole repo).',
        value: summary.rootVar,
      },
      {
        label: summary.branchVar,
        description: 'Tracked branch for the current attempt.',
        value: summary.branchVar,
      },
      {
        label: summary.nameVar,
        description: 'Display name configured in project settings.',
        value: summary.nameVar,
      },
      {
        label: summary.primaryFlagVar,
        description: '"1" when primary, otherwise "0".',
        value: summary.primaryFlagVar,
      },
    ];

    return (
      <div className="space-y-2">
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <span className="font-medium text-foreground">{repo.name}</span>
          <Badge variant="secondary">Prefix {summary.prefix}</Badge>
          {repo.is_primary ? <Badge variant="default">Primary</Badge> : null}
        </div>
        {renderTokens(repoTokens)}
      </div>
    );
  };

  const renderContent = () => {
    if (selectedOption.value === 'primary' || !('repository' in selectedOption)) {
      return renderPrimaryContext();
    }
    return renderRepositoryContext(selectedOption);
  };

  return (
    <div className="space-y-3 rounded-md border border-dashed border-muted-foreground/40 bg-muted/30 p-4">
      <div className="space-y-1">
        <p className="text-sm font-medium text-foreground">Script environment variables</p>
        <p className="text-xs text-muted-foreground">
          Pick a repository to copy the relevant environment variable names.
        </p>
      </div>
      {options.length > 1 ? (
        <div className="space-y-1">
          <Label htmlFor="script-env-repo" className="text-xs uppercase text-muted-foreground">
            Repository
          </Label>
          <Select
            value={selectedOption.value}
            onValueChange={(value) => {
              const next = options.find((option) => option.value === value);
              if (next) {
                setSelectedOption(next);
              }
            }}
          >
            <SelectTrigger id="script-env-repo" className="h-9 w-full text-sm sm:w-72">
              <SelectValue placeholder="Select repository" />
            </SelectTrigger>
            <SelectContent position="popper">
              {options.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      ) : null}
      {renderContent()}
    </div>
  );
}
