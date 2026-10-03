#!/usr/bin/env bash
# Tests for release-targets.sh and image-tags.sh (#15): which commits are built, and which tags
# each publishes, above all that :latest only ever goes to the highest stable release.
# Run from anywhere: bash .github/scripts/test-release-scripts.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
failures=0

check() {
  local name=$1 expected=$2 actual=$3
  if [[ "${expected}" == "${actual}" ]]; then
    echo "ok   ${name}"
  else
    echo "FAIL ${name}"
    echo "     expected: ${expected}"
    echo "     actual:   ${actual}"
    failures=$((failures + 1))
  fi
}

# A throwaway repository with the given tags.
repo=$(mktemp -d)
trap 'rm -rf "${repo}"' EXIT
git -C "${repo}" init -q -b master
git -C "${repo}" -c user.email=t@t -c user.name=t commit -q --allow-empty -m one

targets() {
  (cd "${repo}" && EVENT=$1 REF=$2 SHA=abc1234def DEFAULT_BRANCH=master INPUT_REF=${3:-} bash "${here}/release-targets.sh")
}

tags() {
  IMAGE=img KIND=$1 SEMVER=${2:-} TAG=${3:-} SHA=abc1234def BRANCH=master LATEST=${4:-false} bash "${here}/image-tags.sh" | paste -sd' ' -
}

# Before any release.
check 'push to master: one trunk target' '[{"ref":"abc1234def","kind":"trunk","tag":"","latest":false}]' "$(targets push refs/heads/master)"
check 'push to another branch: nothing' '[]' "$(targets push refs/heads/feature)"
check 'schedule with no release: trunk only' '[{"ref":"refs/heads/master","kind":"trunk","tag":"","latest":false}]' "$(targets schedule refs/heads/master)"

git -C "${repo}" tag v0.1.0
check 'first release is latest' '[{"ref":"refs/tags/v0.1.0","kind":"release","tag":"v0.1.0","latest":true}]' "$(targets push refs/tags/v0.1.0)"

git -C "${repo}" tag v0.2.0
git -C "${repo}" tag v0.1.5
git -C "${repo}" tag v0.10.0
git -C "${repo}" tag v1.0.0-rc.1
check 'highest by version, not by text (0.10 > 0.2)' '[{"ref":"refs/tags/v0.10.0","kind":"release","tag":"v0.10.0","latest":true}]' "$(targets push refs/tags/v0.10.0)"
check 'a patch to an older line does not take latest' '[{"ref":"refs/tags/v0.1.5","kind":"release","tag":"v0.1.5","latest":false}]' "$(targets push refs/tags/v0.1.5)"
check 'a pre-release never takes latest' '[{"ref":"refs/tags/v1.0.0-rc.1","kind":"release","tag":"v1.0.0-rc.1","latest":false}]' "$(targets push refs/tags/v1.0.0-rc.1)"
check 'schedule: trunk and the newest stable release' \
  '[{"ref":"refs/heads/master","kind":"trunk","tag":"","latest":false},{"ref":"refs/tags/v0.10.0","kind":"release","tag":"v0.10.0","latest":true}]' \
  "$(targets schedule refs/heads/master)"
check 'dispatch, default: trunk' '[{"ref":"refs/heads/master","kind":"trunk","tag":"","latest":false}]' "$(targets workflow_dispatch refs/heads/feature)"
check 'dispatch of a tag' '[{"ref":"refs/tags/v0.2.0","kind":"release","tag":"v0.2.0","latest":false}]' "$(targets workflow_dispatch refs/heads/master v0.2.0)"
check 'dispatch of anything else fails' 'failed' "$(targets workflow_dispatch refs/heads/master feature 2>/dev/null || echo failed)"

check 'trunk tags' 'img:edge img:master img:sha-abc1234 img:0.1.1-3' "$(tags trunk 0.1.1-3)"
check 'latest release tags' 'img:0.1.0 img:0.1 img:latest img:sha-abc1234' "$(tags release 0.1.0 v0.1.0 true)"
check 'older-line release tags: no latest' 'img:0.1.5 img:0.1 img:sha-abc1234' "$(tags release 0.1.5 v0.1.5 false)"
check 'pre-release tags: version and sha only' 'img:1.0.0-rc.1 img:sha-abc1234' "$(tags release 1.0.0-rc.1 v1.0.0-rc.1 true)"
check 'no :0 major alias' '' "$(tags release 0.1.0 v0.1.0 true | tr ' ' '\n' | grep -x 'img:0' || true)"

if (( failures > 0 )); then
  echo "${failures} failed"
  exit 1
fi
echo 'all passed'
