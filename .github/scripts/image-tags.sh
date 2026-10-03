#!/usr/bin/env bash
# The tags a release run publishes for one image (#15), one per line, as IMAGE:TAG.
#
#   trunk     edge, <branch>, sha-<short>, <semVer>
#   release   <version>, <major>.<minor> (stable only), sha-<short>, latest (when LATEST=true)
#
# No :<major> alias: while the major is 0 it would float across breaking minors. A pre-release
# tag (v1.0.0-rc.1) publishes its own version and sha only.
# Environment: IMAGE, KIND, SEMVER (GitVersion), TAG (release only, e.g. v0.1.0), SHA, BRANCH,
# LATEST.
set -euo pipefail

short=${SHA:0:7}
tags=()
case "${KIND}" in
  trunk)
    tags+=(edge "${BRANCH}" "sha-${short}" "${SEMVER}")
    ;;
  release)
    version=${TAG#v}
    tags+=("${version}")
    if [[ "${version}" =~ ^([0-9]+)\.([0-9]+)\.[0-9]+$ ]]; then
      tags+=("${BASH_REMATCH[1]}.${BASH_REMATCH[2]}")
      if [[ "${LATEST}" == "true" ]]; then tags+=(latest); fi
    fi
    tags+=("sha-${short}")
    ;;
  *)
    echo "::error::KIND must be trunk or release, not '${KIND}'" >&2
    exit 1
    ;;
esac

for tag in "${tags[@]}"; do
  echo "${IMAGE}:${tag}"
done
