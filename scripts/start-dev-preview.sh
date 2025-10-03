#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ENV_FILE="${ROOT}/frontend/.env.local"
BACKUP_FILE=""
PIPE=""
DEV_PID=""
PARSER_PID=""
CLEANED_UP=0

restore_env_file() {
  if [[ -n "${BACKUP_FILE}" && -f "${BACKUP_FILE}" ]]; then
    mv "${BACKUP_FILE}" "${ENV_FILE}"
  else
    rm -f "${ENV_FILE}"
  fi
}

cleanup() {
  if [[ ${CLEANED_UP} -eq 1 ]]; then
    return
  fi
  CLEANED_UP=1

  if [[ -n "${PARSER_PID}" ]]; then
    kill "${PARSER_PID}" 2>/dev/null || true
    wait "${PARSER_PID}" 2>/dev/null || true
  fi

  if [[ -n "${DEV_PID}" ]]; then
    kill "${DEV_PID}" 2>/dev/null || true
    wait "${DEV_PID}" 2>/dev/null || true
  fi

  if [[ -n "${PIPE}" ]]; then
    rm -f "${PIPE}"
  fi

  restore_env_file
}
trap cleanup EXIT INT TERM

HOST_RAW="$(hostname)"
HOST_LOWER="$(printf '%s' "${HOST_RAW}" | tr '[:upper:]' '[:lower:]')"
HOST_SHORT="${HOST_LOWER%%.*}"

if [[ -f "${ENV_FILE}" ]]; then
  BACKUP_FILE="$(mktemp "${ENV_FILE}.bak.XXXXXX")"
  cp "${ENV_FILE}" "${BACKUP_FILE}"
fi

cat >"${ENV_FILE}" <<EOF_ENV
VITE_STRICT_ALLOWED_HOSTS=false
VITE_ALLOWED_HOSTS=localhost,127.0.0.1,${HOST_RAW},${HOST_LOWER},${HOST_SHORT},gmac,GMAC
EOF_ENV

strip_ansi() {
  sed -E $'s/\x1B\[[0-9;]*[[:alpha:]]//g'
}

normalize_host() {
  local host="${1}"
  case "${host}" in
    ""|0.0.0.0|127.0.0.1|localhost|::|::1|"[::]"|"[::1]")
      printf '127.0.0.1'
      ;;
    *)
      if [[ "${host}" == \[*\] ]]; then
        printf '%s' "${host:1:${#host}-2}"
      else
        printf '%s' "${host}"
      fi
      ;;
  esac
}

normalize_url() {
  local url="${1}"
  if [[ "${url}" =~ ^(https?://)([^/]+)(.*)$ ]]; then
    local scheme="${BASH_REMATCH[1]}"
    local hostport="${BASH_REMATCH[2]}"
    local rest="${BASH_REMATCH[3]}"
    if [[ "${hostport}" =~ ^(\[[^]]+\]|[^:]+)(:[0-9]{2,5})?$ ]]; then
      local host="${BASH_REMATCH[1]}"
      local port="${BASH_REMATCH[2]}"
      host="$(normalize_host "${host}")"
      printf '%s%s%s%s\n' "${scheme}" "${host}" "${port}" "${rest}"
      return 0
    fi
  fi
  printf '%s\n' "${url}"
}

extract_url() {
  local line="${1}"

  if [[ "${line}" =~ (Server|Preview|Local)[[:space:]]*:[[:space:]]*(https?://[^[:space:]]+) ]]; then
    normalize_url "${BASH_REMATCH[2]}"
    return 0
  fi

  if [[ "${line}" =~ (https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::\]|\[::1\]|::|::1|[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)(?::[0-9]{2,5})?[^[:space:]]*) ]]; then
    normalize_url "${BASH_REMATCH[1]}"
    return 0
  fi

  if [[ "${line}" =~ (localhost|127\.0\.0\.1|0\.0\.0\.0|\[::\]|\[::1\]|::|::1):([0-9]{2,5}) ]]; then
    local host="$(normalize_host "${BASH_REMATCH[1]}")"
    local port="${BASH_REMATCH[2]}"
    printf 'http://%s:%s\n' "${host}" "${port}"
    return 0
  fi

  return 1
}

forward_logs() {
  local printed=0
  local raw_line
  local url
  while IFS= read -r raw_line; do
    printf '%s\n' "${raw_line}"
    if [[ ${printed} -eq 0 ]]; then
      local stripped
      stripped="$(printf '%s' "${raw_line}" | strip_ansi)"
      url=""
      if url=$(extract_url "${stripped}"); then
        printf 'Server: %s\n' "${url}"
        printed=1
      fi
    fi
  done
}

PIPE="$(mktemp -t vibe-dev-stream.XXXXXX)"
rm -f "${PIPE}"
mkfifo "${PIPE}"

forward_logs <"${PIPE}" &
PARSER_PID=$!

if command -v stdbuf >/dev/null 2>&1; then
  DEV_CMD=(stdbuf -oL -eL npm run dev)
else
  DEV_CMD=(npm run dev)
fi

"${DEV_CMD[@]}" >"${PIPE}" 2>&1 &
DEV_PID=$!

wait "${DEV_PID}"
DEV_STATUS=$?

wait "${PARSER_PID}" 2>/dev/null || true

exit ${DEV_STATUS}
