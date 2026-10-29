# Releasing Bancwr

## Version authority

A release's version is set by its **release PR**. [release-please](https://github.com/googleapis/release-please)
(`.github/workflows/release.yml`, `release-please-config.json`, `.release-please-manifest.json`)
reads the Conventional Commits merged to `master` since the last tag, works out the next version,
and keeps one pull request, `chore(release): <version>`, open with that version. Merging it creates
the `v<version>` tag. This is the same process as Porwr's, and the Diogel release-process standard
(`40-engineering/standards/release-process.md` in diogel-io/workspace) describes it for every
product.

GitVersion (`GitVersion.yml`) versions trunk builds only: the `:edge` images and their `<semVer>`
tag, computed from the latest `v*` tag with the GitHub Flow preset (one protected branch,
`master`, short-lived branches, no `develop`).

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
| `master`, before any release | `0.1.0-49` (`next-version`, then commits since the last tag) |
| The `v0.1.0` tag | `0.1.0` |
| `master`, after `v0.1.0` | `0.1.1-1` |

GitVersion always counts on from the last tag's patch, so a trunk build can read `0.1.1-3` while the
open release PR says `0.2.0`. The release PR is the one that counts; trunk versions only order
`:edge` builds.

Images are tagged with `semVer`, not `fullSemVer`. `fullSemVer` can carry `+` build metadata,
and image tags cannot contain `+`.

To check the version locally without installing .NET:

```bash
podman run --rm -v "$PWD:/repo:Z" docker.io/gittools/gitversion:6.8.2 /repo /showvariable SemVer
```

## Image tags

`:latest` is the newest reviewed release, and trunk is `:edge` (#15). Before #15 every push to
`master` wrote `:latest`, so a self-hoster following it ran whatever merged last. The last of those
images (`0.1.0-99`) stayed in GHCR as `:latest` after #15 until it was deleted under #90. Before
the first release, neither package should have a `latest` tag at all.

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

A release's version is its tag. A trunk build's is GitVersion's, which in Actions follows the branch
the run was started from: started from another branch (`--ref`), a preview of `master` shows that
branch's label, such as `0.1.0-my-branch.1`. Runs on `master` itself are not affected.

## Cutting a release

Releases are cut by merging the release PR. Nobody pushes a tag by hand.

### Scope: the milestone

Each planned release has a GitHub milestone named after its version without the `v`, for example
`0.1.0`. Issues are put in the milestone when they are deliberately scoped into that release, and
every pull request that delivers one is put in the same milestone. The release PR is put in it too,
so the milestone lists the release's scope, the changes that delivered it, and the PR that ships
it. Create the next milestone only when there is work to put in it. The release workflow closes the
milestone when the release is created.

### The release PR

On every push to `master`, release-please opens or updates `chore(release): <version>` from the
branch `release-please--branches--master--components--bancwr`, labelled `autorelease: pending`. It:

- sets the version from the commits since the last tag (see [Choosing the next version](#choosing-the-next-version));
- writes the new section of `CHANGELOG.md` from the `feat`, `fix`, `perf` and `revert` commits;
- bumps both images in `compose.yaml`, on the lines marked `# x-release-please-version`, so the
  tagged commit's `compose.yaml` names its own release.

A merge with no `feat`, `fix`, `perf`, `revert` or breaking commit (for example `docs` or `ci`
alone) leaves the release PR as it is, or opens none.

The release PR is opened with the workflow `GITHUB_TOKEN`, so GitHub does not run CI on it. Close
and reopen it, or push an empty commit to its branch, to get the checks.

### Releasing

1. Check that every issue in the version's milestone is closed or moved to a later milestone.
2. Review the release PR: the version, the `CHANGELOG.md` section and the `compose.yaml` bump. If a
   commit is in the wrong section (a breaking change without `!`, say), fix the commit message
   rather than editing the generated PR: see [Fixing the release notes](#fixing-the-release-notes).
3. Merge the release PR. `.github/workflows/release.yml` then:
   - creates the `v<version>` tag on the merge commit and the GitHub release, with the changelog
     section as its notes, and relabels the PR `autorelease: tagged`;
   - starts **Release Backend** and **Release Frontend** for the tag (by `workflow_dispatch`,
     because a tag created with the workflow token does not start `on: push: tags` workflows);
   - closes the milestone named after the version.
4. Wait for **Release Backend** and **Release Frontend** to finish on the tag and confirm both
   images carry `0.1.0`, `0.1` and `latest` in GHCR, and that `latest` and `0.1.0` have the same
   digest. Each runs the full test suite first (see [The test gate](#the-test-gate)); if a test
   fails, nothing is published and the job graph shows `test` failed before `release`. Fix the
   cause, then run the release by hand for the tag:

   ```bash
   gh workflow run "Release Backend" -R diogel-io/bancwr -f ref=v0.1.0
   gh workflow run "Release Frontend" -R diogel-io/bancwr -f ref=v0.1.0
   ```

A tag pushed by hand still starts both image releases, as before, but it bypasses the release PR,
the changelog and the version bump. Use it only to recover from a broken release run.

### Fixing the release notes

release-please reads commit messages, not PR titles. To correct one after it has merged, edit the
merged PR's body to add an override block, and the next run regenerates the release PR from it:

```text
BEGIN_COMMIT_OVERRIDE
feat!: replace the roles with admin, signer and viewer
END_COMMIT_OVERRIDE
```

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

### Who releases

Merging the release PR is the release. #38 asked for a person to be accountable for each release of
a component that holds signing keys: that person is whoever approves and merges the release PR. It
is reviewed like any other change, and nothing is tagged or published until it is merged.

### Choosing the next version

release-please chooses the version from the commit types since the last tag. While the major
version is 0 (`bump-minor-pre-major`):

| Commit | Next version |
| --- | --- |
| `fix:` | patch, `0.1.0` to `0.1.1` |
| `feat:` | minor, `0.1.0` to `0.2.0` |
| `feat!:` or a `BREAKING CHANGE:` footer | minor as well while the major is 0 |

To force a version, such as `1.0.0`, merge a commit whose message has a `Release-As: 1.0.0` footer.
The manifest starts at `0.0.0` and the commit that set up release-please carries
`Release-As: 0.1.0`, so the first release is `0.1.0`.
