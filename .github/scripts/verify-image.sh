#!/usr/bin/env bash
# Verifies a published Bancwr release image (#104): its cosign signature, its SLSA build provenance
# and its SPDX SBOM. release-image.yml made all three, keyless, under its own GitHub OIDC identity.
# The release workflow runs this on every release image it publishes, and an operator can run it
# before deploying one. docs/releasing.md, "Verifying an image", gives the same commands.
#
#   bash .github/scripts/verify-image.sh ghcr.io/diogel-io/bancwr-diogel-backend@sha256:<digest>
#
# Takes a digest, never a tag: a tag can be moved after it is checked. Needs cosign 3 or later and
# gh, signed in. Exits non-zero if any check fails.
set -euo pipefail

# The one place these are written down. test-release-scripts.sh checks that docs/releasing.md
# gives the same identity.
REPOSITORY='diogel-io/bancwr'
OIDC_ISSUER='https://token.actions.githubusercontent.com'
# The release workflow runs from master (started by release.yml, or the weekly rebuild), or from
# the tag itself when a tag is pushed by hand to recover a release. All three checks pin this
# identity: the workflow file and the ref it ran from, so a run from any other branch fails.
IDENTITY_REGEXP='^https://github\.com/diogel-io/bancwr/\.github/workflows/release-image\.yml@refs/(heads/master|tags/v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?)$'
SBOM_PREDICATE='https://spdx.dev/Document/v2.3'

if [[ $# -ne 1 || "$1" != ghcr.io/diogel-io/bancwr-diogel-*@sha256:* ]]; then
  echo "usage: $0 ghcr.io/diogel-io/bancwr-diogel-<backend|frontend>@sha256:<digest>" >&2
  exit 2
fi
image=$1

echo "Signature: ${image}"
cosign verify "${image}" \
  --certificate-oidc-issuer "${OIDC_ISSUER}" \
  --certificate-identity-regexp "${IDENTITY_REGEXP}" \
  > /dev/null

echo "Build provenance: ${image}"
gh attestation verify "oci://${image}" \
  --repo "${REPOSITORY}" \
  --cert-oidc-issuer "${OIDC_ISSUER}" \
  --cert-identity-regex "${IDENTITY_REGEXP}"

echo "SBOM: ${image}"
gh attestation verify "oci://${image}" \
  --repo "${REPOSITORY}" \
  --cert-oidc-issuer "${OIDC_ISSUER}" \
  --cert-identity-regex "${IDENTITY_REGEXP}" \
  --predicate-type "${SBOM_PREDICATE}"

echo "Verified ${image}"
