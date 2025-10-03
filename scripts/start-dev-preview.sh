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

is_loopback_host() {
  case "$1" in
    ""|localhost|127.0.0.1|0.0.0.0|::|::1|"[::]"|"[::1]")
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

normalize_host() {
  local host="${1}"
  if [[ "${host}" == \[*\] ]]; then
    host="${host:1:${#host}-2}"
  fi
  if is_loopback_host "${host}"; then
    printf '127.0.0.1'
  else
    printf '%s' "${host}"
  fi
}

resolve_preview_host() {
  local candidate
  for candidate in \
    "${PREVIEW_OUTPUT_HOST:-}" \
    "${BACKEND_HOST:-}" \
    "${HOST_SHORT:-}" \
    "${HOST_LOWER:-}" \
    "${HOST_RAW:-}"; do
    if [[ -z "${candidate}" ]]; then
      continue
    fi
    if is_loopback_host "${candidate}"; then
      continue
    fi
    printf '%s\n' "${candidate}"
    return
  done
  printf '127.0.0.1\n'
}

PREFERRED_PREVIEW_HOST="$(resolve_preview_host)"

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

rewrite_preview_url() {
  local url="${1}"
  if [[ "${url}" != *://* ]]; then
    printf '%s\n' "${url}"
    return
  fi

  local scheme="${url%%://*}"
  local rest="${url#${scheme}://}"
  local hostport="${rest}"
  local path="/"

  if [[ "${rest}" == */* ]]; then
    hostport="${rest%%/*}"
    path="/${rest#*/}"
  fi

  local host="${hostport}"
  local port=""

  if [[ "${hostport}" == \[*\]* ]]; then
    host="${hostport%%]*}"
    host="${host#\[}"
    local remainder="${hostport#*]}"
    if [[ "${remainder}" == :* ]]; then
      port="${remainder#:}"
    fi
  elif [[ "${hostport}" == *:* ]]; then
    host="${hostport%%:*}"
    port="${hostport##*:}"
  fi

  local output_host="${host}"
  if is_loopback_host "${host}"; then
    output_host="${PREFERRED_PREVIEW_HOST}"
  fi

  if [[ "${output_host}" == *:* && "${output_host}" != \[* && "${output_host}" != *\] ]]; then
    output_host="[${output_host}]"
  fi

  local port_segment=""
  if [[ -n "${port}" ]]; then
    port_segment=":${port}"
  fi

  if [[ -z "${path}" ]]; then
    path="/"
  fi

  printf '%s://%s%s%s\n' "${scheme}" "${output_host}" "${port_segment}" "${path}"
}

extract_url() {
  local line="${1}"

  if [[ "${line}" =~ Server[[:space:]]+running ]]; then
    return 1
  fi

  if [[ "${line}" =~ (Local|Preview|Network)[[:space:]]*:[[:space:]]*(https?://[^[:space:]]+) ]]; then
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
        url="$(rewrite_preview_url "${url}")"
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
