#!/usr/bin/env bash
# Keeps compose.yaml and .env.example in step (#83): every variable .env.example documents must
# reach the service that reads it under compose, with the value .env gives it. NIP46_ENABLED and
# NIP46_RELAYS once never did, so NIP-46 was off on every standard install whatever .env said.
#
# Each documented variable is given a sentinel value, compose renders the file, and each sentinel
# must arrive unchanged: a variable compose drops, or sets to a fixed value of its own, fails.
# Variables under "# Only without compose." in .env.example are the exception: compose must not
# read them from .env at all.
# Run from anywhere: bash .github/scripts/test-compose-env.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
env_example="${root}/.env.example"
compose_file="${root}/compose.yaml"
failures=0

fail() {
  echo "FAIL $1"
  failures=$((failures + 1))
}

# Documented variables, set or commented out (`NAME=` or `# NAME=`), split at the marker line.
documented() {
  awk -v want="$1" '
    /^# Only without compose\./ { section = "without" }
    match($0, /^#? ?[A-Z][A-Z0-9_]*=/) {
      name = substr($0, RSTART, RLENGTH); sub(/^#? ?/, "", name); sub(/=$/, "", name)
      if ((section == "without") == (want == "without")) print name
    }
  ' "${env_example}" | sort -u
}

mapfile -t with_compose < <(documented with)
mapfile -t without_compose < <(documented without)
if [[ ${#with_compose[@]} -eq 0 ]]; then
  echo "FAIL no variables found in .env.example"
  exit 1
fi

# The frontend reads NUXT_*; the bunker everything else. BANCWR_PROXY_SECRET reaches both, as
# NUXT_PROXY_SECRET on the frontend.
targets() {
  case "$1" in
    NUXT_*) echo "frontend $1" ;;
    BANCWR_PROXY_SECRET) echo "bunker $1"; echo "frontend NUXT_PROXY_SECRET" ;;
    *) echo "bunker $1" ;;
  esac
}

sentinel() { echo "sentinel-${1,,}"; }

# Sentinels in the environment override .env.example, so every ${…} compose reads is one of them.
rendered=$(
  for name in "${with_compose[@]}"; do export "${name}=$(sentinel "${name}")"; done
  docker compose -f "${compose_file}" --env-file "${env_example}" config --format json
)

for name in "${with_compose[@]}"; do
  while read -r service variable; do
    actual=$(jq -r --arg s "${service}" --arg v "${variable}" '.services[$s].environment[$v] // "<missing>"' <<<"${rendered}")
    if [[ "${actual}" == "$(sentinel "${name}")" ]]; then
      echo "ok   ${name} reaches ${service} as ${variable}"
    else
      fail "${name}: ${service} gets ${variable}=${actual}, not the value from .env"
    fi
  done < <(targets "${name}")
done

for name in "${without_compose[@]}"; do
  if grep -Eq "\\\$\\{${name}([:?}-]|$)" "${compose_file}"; then
    fail "${name} is marked 'Only without compose' in .env.example, but compose.yaml reads it"
  else
    echo "ok   ${name} is not read by compose"
  fi
done

if [[ ${failures} -gt 0 ]]; then
  echo "${failures} failed: compose.yaml and .env.example disagree."
  exit 1
fi
echo "compose.yaml and .env.example agree."
