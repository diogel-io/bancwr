# Releasing Bancwr

## Version authority

`GitVersion.yml` is the only source of the Bancwr version. GitVersion computes it from that file
and the repository's `v*` tags, using the GitHub Flow preset: one protected branch (`master`),
short-lived branches, no `develop`.

Backend and frontend are released together under one version and one tag namespace,
`v<major>.<minor>.<patch>`. There is no per-image versioning.

The manifests are not release inputs:

- `backend/Cargo.toml` is pinned at `0.0.0`. CI does not rewrite it, because a changed
  `Cargo.toml` changes the cargo-chef recipe and throws away the cached dependency layer on every
  release.
- `frontend/package.json` has no `version` field. The package is private and npm does not need one.

The release workflows pass the computed version into both images as the `BANCWR_VERSION` build
argument, which is set as an environment variable of the same name in the running container.

### Checking the running version

Each image reports its own `BANCWR_VERSION` (#35):

- **Bunker:** the `version` field of `GET /api/bunker/status`, for example
  `curl -s http://localhost:3000/api/bunker/status | jq .version`. The first log line also carries
  it: `Bancwr Diogel 0.1.0-49 starting...`. `/health` does not, and stays `{"status":"ok"}` for
  container probes.
- **Frontend:** `GET /api/version` on the frontend, for example
  `curl -s http://localhost:3001/api/version`.
- **Dashboard:** the sidebar footer shows both, and warns when they differ, which means the two
  containers were not updated together.

`0.0.0` means an unversioned build: `cargo run`, `pnpm dev`, or an image built without the build
argument.

## How the version is computed

`.github/workflows/version.yml` runs GitVersion once per release run and exposes
`majorMinorPatch`, `semVer`, and `fullSemVer`. `release-backend.yml` and `release-frontend.yml`
both call it.

| Where | Example `semVer` |
| --- | --- |
| `master`, before any release | `0.1.0-49` (next version, then commits since the last tag) |
| The `v0.1.0` tag | `0.1.0` |
| `master`, after `v0.1.0` | `0.1.1-1` |

Images are tagged with `semVer`, not `fullSemVer`. `fullSemVer` can carry `+` build metadata,
and image tags cannot contain `+`.

To check the version locally without installing .NET:

```bash
podman run --rm -v "$PWD:/repo:Z" docker.io/gittools/gitversion:6.8.2 /repo /showvariable SemVer
```

## Image tags

Every push to `master` and every `v*` tag publishes both images to GHCR:

| Tag | Written on | Points at |
| --- | --- | --- |
| `latest` | push to `master` | The current `master` commit. Trunk, not a reviewed release. |
| `master` | push to `master` | The same image as `latest`. |
| `sha-<short>` | every run | One specific commit. |
| `<semVer>` | every run | The GitVersion version: `0.1.0-49` from `master`, `0.1.0` from `v0.1.0`. |
| `<version>` | `v*` tag | The released version without the `v` prefix, so `v0.1.0` publishes `0.1.0`. |

Making `latest` release-only and adding an `edge` tag for trunk is tracked in
[#15](https://github.com/diogel-io/bancwr/issues/15).

## Cutting a release

1. Check that every issue in the release's milestone is closed or moved to a later milestone.
2. Update your local `master` and confirm the version GitVersion will produce:

   ```bash
   git switch master
   git pull --ff-only
   podman run --rm -v "$PWD:/repo:Z" docker.io/gittools/gitversion:6.8.2 /repo /showvariable MajorMinorPatch
   ```

3. Create and push an annotated tag from your own account:

   ```bash
   git tag -a v0.1.0 -m "Bancwr 0.1.0"
   git push origin v0.1.0
   ```

4. Wait for **Release Backend** and **Release Frontend** to finish on the tag and confirm both
   images carry the `0.1.0` tag in GHCR. Each runs the full test suite first (see
   [The test gate](#the-test-gate)); if a test fails, nothing is published and the job graph shows
   `test` failed before `release`.
5. Create the GitHub release from the tag with generated notes. `.github/release.yml` groups them
   by pull request label.

   ```bash
   gh release create v0.1.0 --generate-notes --verify-tag
   ```

6. Close the milestone.

### The test gate

`.github/workflows/test.yml` runs `cargo test`, and the frontend's lint, typecheck and
`pnpm test` ([#36](https://github.com/diogel-io/bancwr/issues/36)). Three workflows call it:

- **CI**, on every pull request and every push to `master`. Its `test / backend-test` and
  `test / frontend-check` checks are required before a pull request can merge into `master`.
- **Release Backend** and **Release Frontend**, whose `release` job needs `test`. An image is
  only published for a commit or tag whose tests pass. CI does not run on `v*` tags, so for a
  release tag this is the only gate.

### Push the tag yourself

Release tagging is deliberately manual
([#38](https://github.com/diogel-io/bancwr/issues/38)): a person is accountable for every release
of a component that holds signing keys. Do not add a workflow that creates `v*` tags.

It would not work with the default credential anyway. A tag pushed with the workflow
`GITHUB_TOKEN` does not start `on: push: tags` workflows, because GitHub blocks it to prevent
workflow loops, so a workflow that created the tag would publish nothing.

### Choosing the next version

GitVersion increments the patch after each tag. To make the next release a minor or major
version, raise `next-version` in `GitVersion.yml` in a normal pull request, or add
`+semver: minor` or `+semver: major` to a commit message.

## Milestones

Each planned release has a GitHub milestone named after its version without the `v`, for example
`0.1.0`. Issues are added to a milestone when they are deliberately scoped into that release.
Create the next milestone only when there is work to put in it, and close a milestone when its
tag is published.
