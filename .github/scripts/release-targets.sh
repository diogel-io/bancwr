#!/usr/bin/env bash
# Which commits a release run builds, and as what (#15). Prints a JSON array of
# {"ref": …, "kind": "trunk"|"release", "tag": "v…"|"", "latest": true|false}.
#
#   push to the default branch   one trunk target (:edge)
#   push of a v* tag             one release target; latest only if it is the highest stable tag
#   schedule                     the default branch, and the newest stable v* tag if there is one
#   workflow_dispatch            INPUT_REF: the default branch (empty means it) or a v* tag
#
# Anything else (another branch) yields no targets, so nothing is published from it.
# Environment: EVENT, REF (github.ref), SHA, DEFAULT_BRANCH, INPUT_REF. Tags are read from git,
# so the checkout must have them (fetch-depth: 0).
set -euo pipefail

stable_tags() {
  git tag --list 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V || true
}

# The highest stable release tag, or nothing.
highest() {
  stable_tags | tail -n 1
}

trunk() {
  printf '{"ref":"%s","kind":"trunk","tag":"","latest":false}' "$1"
}

release() {
  local tag=$1 latest=false
  if [[ "$tag" == "$(highest)" ]]; then latest=true; fi
  printf '{"ref":"refs/tags/%s","kind":"release","tag":"%s","latest":%s}' "$tag" "$tag" "$latest"
}

targets=()
case "${EVENT}" in
  push)
    if [[ "${REF}" == "refs/heads/${DEFAULT_BRANCH}" ]]; then
      targets+=("$(trunk "${SHA}")")
    elif [[ "${REF}" == refs/tags/v* ]]; then
      targets+=("$(release "${REF#refs/tags/}")")
    fi
    ;;
  schedule)
    targets+=("$(trunk "refs/heads/${DEFAULT_BRANCH}")")
    newest=$(highest)
    if [[ -n "${newest}" ]]; then targets+=("$(release "${newest}")"); fi
    ;;
  workflow_dispatch)
    ref=${INPUT_REF:-${DEFAULT_BRANCH}}
    ref=${ref#refs/heads/}
    ref=${ref#refs/tags/}
    if [[ "${ref}" == "${DEFAULT_BRANCH}" ]]; then
      targets+=("$(trunk "refs/heads/${DEFAULT_BRANCH}")")
    elif git rev-parse -q --verify "refs/tags/${ref}" >/dev/null && [[ "${ref}" == v* ]]; then
      targets+=("$(release "${ref}")")
    else
      echo "::error::ref must be ${DEFAULT_BRANCH} or an existing v* tag, not '${ref}'" >&2
      exit 1
    fi
    ;;
esac

(IFS=,; printf '[%s]\n' "${targets[*]:-}")
