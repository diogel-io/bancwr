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

`:latest` is the newest reviewed release, and trunk is `:edge` (#15). Before #15 every push to
`master` wrote `:latest`, so a self-hoster following it ran whatever merged last.

| Tag | Written on | Points at |
| --- | --- | --- |
| `latest` | a `v*` tag that is the highest stable release; the weekly rebuild of that release | The newest release. |
| `<version>` | a `v*` tag; the weekly rebuild of the newest release | That release, so `v0.1.0` publishes `0.1.0`. |
| `<major>.<minor>` | a stable `v*` tag | The newest patch of that line, such as `0.1`. |
| `edge` | push to `master`; the weekly rebuild of `master` | Trunk. Not a reviewed release. |
| `master` | the same | The same image as `edge`. |
| `<semVer>` | push to `master`; the weekly rebuild of `master` | The GitVersion version of a trunk build, such as `0.1.1-3`. |
| `sha-<short>` | every run | One specific commit. |

The rules live in two scripts, tested by `.github/scripts/test-release-scripts.sh` (CI's
`release-scripts` job): `release-targets.sh` decides which commits a run builds, and
`image-tags.sh` which tags each gets. `release-image.yml` builds, gates and publishes one image for
one target; `release-backend.yml` and `release-frontend.yml` only call it.

- **`latest` only moves forward.** It goes to a tag only if that tag is the highest stable
  `v<major>.<minor>.<patch>` by version (`0.10.0` is above `0.2.0`). A patch to an older line, such
  as `v0.1.5` after `v0.2.0`, publishes `0.1.5` and `0.1` and leaves `latest` alone. A
  pre-release tag (`v1.0.0-rc.1`) publishes its version and `sha-` only.
- **There is no `:<major>` alias**: while the major version is 0, `:0` would float across
  breaking releases.
- **A rebuilt release keeps its version, not its digest.** The weekly rebuild republishes the
  newest release's tags on fresh base images, from the same commit. Pin by digest only if you also
  want to miss those fixes.

### Seeing what a run would publish

Run **Release Backend** or **Release Frontend** by hand with **publish** off. It builds, tests and
scans, prints the tags it would push, and pushes nothing. **ref** is `master` (or empty) or a `v*`
tag to rebuild:

```bash
gh workflow run "Release Backend" -R diogel-io/bancwr -f publish=false
gh workflow run "Release Backend" -R diogel-io/bancwr -f publish=false -f ref=v0.1.0
```

## Cutting a release

1. Check that every issue in the release's milestone is closed or moved to a later milestone.
2. Pin `compose.yaml` to the new version, for both images, in a pull request of its own, and merge
   it. The tagged commit's `compose.yaml` then names its own release, so a checkout of the tag runs
   it. The tag run warns (it does not fail) when they differ.
3. Update your local `master` and confirm the version GitVersion will produce:

   ```bash
   git switch master
   git pull --ff-only
   podman run --rm -v "$PWD:/repo:Z" docker.io/gittools/gitversion:6.8.2 /repo /showvariable MajorMinorPatch
   ```

4. Create and push an annotated tag from your own account:

   ```bash
   git tag -a v0.1.0 -m "Bancwr 0.1.0"
   git push origin v0.1.0
   ```

5. Wait for **Release Backend** and **Release Frontend** to finish on the tag and confirm both
   images carry `0.1.0`, `0.1` and `latest` in GHCR, and that `latest` and `0.1.0` have the same
   digest. Each runs the full test suite first (see [The test gate](#the-test-gate)); if a test
   fails, nothing is published and the job graph shows `test` failed before `release`.
6. Create the GitHub release from the tag with generated notes. `.github/release.yml` groups them
   by pull request label.

   ```bash
   gh release create v0.1.0 --generate-notes --verify-tag
   ```

7. Close the milestone.

### The vulnerability gate

Both images are distroless (#16): the bunker on `gcr.io/distroless/cc-debian13:nonroot`, the
frontend on `gcr.io/distroless/nodejs24-debian13:nonroot`. Neither has a shell, a package manager,
perl, curl or npm, and both run as UID 65532.

Trivy scans the image itself, not only the source, before anything is published:

| Where | Scans | Code-scanning category | Blocks? |
| --- | --- | --- | --- |
| CI `build`, every pull request and push | The image that would ship | `trivy-image-backend`, `trivy-image-frontend` | Yes, on a fixable critical or high |
| CI `build` | The lockfiles, build-time dependencies included | `trivy-fs-backend`, `trivy-fs-frontend` | No, report only |
| Release workflows, before pushing | The image about to be published | (table in the job log) | Yes, on a fixable critical or high |
| `trivy-security.yml`, Mondays 06:00 UTC | The published `:latest`, the newest release, as users pull it | `trivy-published-backend`, `trivy-published-frontend` | No, report only |
| `trivy-security.yml`, Mondays 06:00 UTC | The published `:edge`, trunk | `trivy-edge-backend`, `trivy-edge-frontend` | No, report only |

Each scan has its own category. They used to share `trivy-backend` and `trivy-frontend`, so every
CI run replaced the published-image results and the base-image findings vanished from view.

The gate fails on a critical or high finding **that has a fix**. A finding with no fix yet is
reported but does not block, because nothing can be done about it until one exists. To accept a
fixable finding instead, add its ID to `.trivyignore` with a comment giving the reason and a review
date, in a pull request of its own.

### Weekly rebuild

Both release workflows also run every Monday at 05:00 UTC. Each rebuilds **two targets** with fresh
base images (`pull: true`), each from its own commit and through its own tests and vulnerability
gate:

- **`master`**, republishing `edge`, `master`, `sha-<short>` and its `<semVer>`;
- **the newest stable release**, once there is one, republishing its version, `<major>.<minor>`
  and `latest`.

The versions do not change, only the digests: a base-image fix reaches what people run within a
week without anyone merging or releasing anything. If a target's gate fails, nothing is published
for that target and the run is red. Before the first release only `master` is rebuilt, and the
weekly scan skips `:latest` with a notice. A run never publishes any other branch.

**GitHub disables scheduled workflows after 60 days without repository activity** (it happened to
the Trivy scan before #8). A quiet period therefore silently stops both the rebuild and the scan.
Check now and then:

```bash
gh workflow list --all -R diogel-io/bancwr
```

A workflow shown as `disabled_inactivity` is re-enabled with
`gh workflow enable "Release Backend" -R diogel-io/bancwr`, and likewise for the others.

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
