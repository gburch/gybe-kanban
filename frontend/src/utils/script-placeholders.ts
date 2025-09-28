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
      setup: String.raw`@echo off
setlocal enabledelayedexpansion

if defined VIBE_PRIMARY_REPO_ROOT (
  cd /d "%VIBE_PRIMARY_REPO_PATH%\%VIBE_PRIMARY_REPO_ROOT%"
) else (
  cd /d "%VIBE_PRIMARY_REPO_PATH%"
)

REM Add setup commands
`,
      dev: String.raw`@echo off
setlocal enabledelayedexpansion

if defined VIBE_PRIMARY_REPO_ROOT (
  cd /d "%VIBE_PRIMARY_REPO_PATH%\%VIBE_PRIMARY_REPO_ROOT%"
) else (
  cd /d "%VIBE_PRIMARY_REPO_PATH%"
)

REM Start your dev server
`,
      cleanup: String.raw`@echo off
setlocal enabledelayedexpansion

if defined VIBE_PRIMARY_REPO_ROOT (
  cd /d "%VIBE_PRIMARY_REPO_PATH%\%VIBE_PRIMARY_REPO_ROOT%"
) else (
  cd /d "%VIBE_PRIMARY_REPO_PATH%"
)

REM Add cleanup commands
`,
    };
  }
}

class UnixScriptPlaceholderStrategy implements ScriptPlaceholderStrategy {
  getPlaceholders(): ScriptPlaceholders {
    return {
      setup: `#!/bin/bash
set -euo pipefail

cd "\${VIBE_PRIMARY_REPO_PATH}/\${VIBE_PRIMARY_REPO_ROOT:-}"

# add setup commands
`,
      dev: `#!/bin/bash
set -euo pipefail

cd "\${VIBE_PRIMARY_REPO_PATH}/\${VIBE_PRIMARY_REPO_ROOT:-}"

# start your dev server
`,
      cleanup: `#!/bin/bash
set -euo pipefail

cd "\${VIBE_PRIMARY_REPO_PATH}/\${VIBE_PRIMARY_REPO_ROOT:-}"

# add cleanup commands
`,
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
