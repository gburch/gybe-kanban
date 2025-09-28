interface ScriptPlaceholders {
  setup: string;
  dev: string;
  cleanup: string;
}

interface ScriptPlaceholderStrategy {
  getPlaceholders(): ScriptPlaceholders;
}

class WindowsScriptPlaceholderStrategy implements ScriptPlaceholderStrategy {
  getPlaceholders(): ScriptPlaceholders {
    return {
      setup: `@echo off
setlocal enabledelayedexpansion

REM Setup scripts start in the primary repository's worktree.
if defined VIBE_PRIMARY_REPO_ROOT (
  cd /d "%VIBE_PRIMARY_REPO_PATH%\%VIBE_PRIMARY_REPO_ROOT%"
) else (
  cd /d "%VIBE_PRIMARY_REPO_PATH%"
)

npm install
REM Add any setup commands here...

REM Example: iterate through extra repositories (uncomment to use)
REM for %%P in (%VIBE_REPOSITORIES:,= %) do (
REM   set "repoPathVar=VIBE_REPO_%%P_PATH"
REM   for /f "tokens=1,* delims==" %%A in ('set !repoPathVar!') do set "repoPath=%%B"
REM   if defined repoPath (
REM     pushd "!repoPath!"
REM     npm install
REM     popd
REM   )
REM )`,
      dev: `@echo off
setlocal enabledelayedexpansion

REM Primary repository path helpers
if defined VIBE_PRIMARY_REPO_ROOT (
  cd /d "%VIBE_PRIMARY_REPO_PATH%\%VIBE_PRIMARY_REPO_ROOT%"
) else (
  cd /d "%VIBE_PRIMARY_REPO_PATH%"
)

npm run dev
REM Add dev server start command here...
REM Use %VIBE_REPOSITORIES% to target additional repos if needed.`,
      cleanup: `@echo off
setlocal enabledelayedexpansion

REM Cleanup scripts run only when the agent made changes.
if defined VIBE_PRIMARY_REPO_ROOT (
  cd /d "%VIBE_PRIMARY_REPO_PATH%\%VIBE_PRIMARY_REPO_ROOT%"
) else (
  cd /d "%VIBE_PRIMARY_REPO_PATH%"
)

REM Add cleanup commands here...
REM Example: call npm test or formatters across repositories.
REM for %%P in (%VIBE_REPOSITORIES:,= %) do (
REM   set "repoPathVar=VIBE_REPO_%%P_PATH"
REM   for /f "tokens=1,* delims==" %%A in ('set !repoPathVar!') do set "repoPath=%%B"
REM   if defined repoPath (
REM     pushd "!repoPath!"
REM     npm test
REM     popd
REM   )
REM )`,
    };
  }
}

class UnixScriptPlaceholderStrategy implements ScriptPlaceholderStrategy {
  getPlaceholders(): ScriptPlaceholders {
    return {
      setup: `#!/bin/bash
set -euo pipefail

# Setup scripts run from the primary repository worktree.
cd "\${VIBE_PRIMARY_REPO_PATH}/\${VIBE_PRIMARY_REPO_ROOT:-}"

npm install
# Add any setup commands here...

# Example: iterate through additional repositories (uncomment to use)
# for prefix in \${VIBE_REPOSITORIES//,/ }; do
#   path_var="VIBE_REPO_\${prefix}_PATH"
#   root_var="VIBE_REPO_\${prefix}_ROOT"
#   repo_path="\${!path_var}"
#   repo_root="\${!root_var}"
#   [ -n "\${repo_path}" ] || continue
#   (cd "\${repo_path}/\${repo_root:-}" && npm install)
# done`,
      dev: `#!/bin/bash
set -euo pipefail

# Start in the primary repository. Use VIBE_REPOSITORIES to target others.
cd "\${VIBE_PRIMARY_REPO_PATH}/\${VIBE_PRIMARY_REPO_ROOT:-}"

npm run dev
# Add dev server start command here...`,
      cleanup: `#!/bin/bash
set -euo pipefail

# Cleanup scripts run only when the agent changed files.
cd "\${VIBE_PRIMARY_REPO_PATH}/\${VIBE_PRIMARY_REPO_ROOT:-}"

# Add cleanup commands here...
# Example: run tests or formatters across repositories.
# for prefix in \${VIBE_REPOSITORIES//,/ }; do
#   path_var="VIBE_REPO_\${prefix}_PATH"
#   root_var="VIBE_REPO_\${prefix}_ROOT"
#   repo_path="\${!path_var}"
#   repo_root="\${!root_var}"
#   [ -n "\${repo_path}" ] || continue
#   (cd "\${repo_path}/\${repo_root:-}" && npm test)
# done`,
    };
  }
}

class ScriptPlaceholderContext {
  private strategy: ScriptPlaceholderStrategy;

  constructor(strategy: ScriptPlaceholderStrategy) {
    this.strategy = strategy;
  }

  setStrategy(strategy: ScriptPlaceholderStrategy): void {
    this.strategy = strategy;
  }

  getPlaceholders(): ScriptPlaceholders {
    return this.strategy.getPlaceholders();
  }
}

export function createScriptPlaceholderStrategy(
  osType: string
): ScriptPlaceholderStrategy {
  if (osType.toLowerCase().includes('windows')) {
    return new WindowsScriptPlaceholderStrategy();
  }
  return new UnixScriptPlaceholderStrategy();
}

export { ScriptPlaceholderContext, type ScriptPlaceholders };
