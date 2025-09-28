import type { ProjectRepository } from 'shared/types';

const NON_ALPHANUMERIC = /[^a-z0-9]+/g;

function gitBranchId(input: string): string {
  const lowered = input.toLowerCase();
  const replaced = lowered.replace(NON_ALPHANUMERIC, '-');
  const trimmed = replaced.replace(/^-+|-+$/g, '');
  const sliced = trimmed.slice(0, 16);
  return sliced.replace(/-+$/g, '');
}

function shortUuid(id: string): string {
  const compact = id.replace(/-/g, '');
  return compact.slice(0, 4);
}

export function getRepositorySlug(repo: ProjectRepository): string {
  const base = gitBranchId(repo.name);
  const suffix = shortUuid(repo.id);
  if (!base) {
    return `repo-${suffix}`;
  }
  return `${base}-${suffix}`;
}

export function getRepositoryEnvPrefix(repo: ProjectRepository): string {
  const slug = getRepositorySlug(repo);
  return slug.replace(/-/g, '_').toUpperCase();
}

export function getRepositoryEnvSummary(repo: ProjectRepository) {
  const prefix = getRepositoryEnvPrefix(repo);
  return {
    prefix,
    pathVar: `VIBE_REPO_${prefix}_PATH`,
    rootVar: `VIBE_REPO_${prefix}_ROOT`,
    branchVar: `VIBE_REPO_${prefix}_BRANCH`,
    nameVar: `VIBE_REPO_${prefix}_NAME`,
    primaryFlagVar: `VIBE_REPO_${prefix}_IS_PRIMARY`,
  } as const;
}
