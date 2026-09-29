// The frontend image's own release version. The release workflows pass the GitVersion semVer in
// as BANCWR_VERSION, which the Dockerfile sets only in the runtime stage, so it has to be read
// when a request arrives: a runtimeConfig value would be fixed at build time, before it exists.
// See docs/releasing.md and diogel-io/bancwr#35.

/** Reported when BANCWR_VERSION is unset or blank, as under `pnpm dev`. Matches the Dockerfile. */
export const DEFAULT_VERSION = '0.0.0'

export function versionOrDefault(value: string | undefined): string {
  return value?.trim() || DEFAULT_VERSION
}
