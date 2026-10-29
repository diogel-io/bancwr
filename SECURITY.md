# Security Policy

Bancwr holds a Nostr private key and signs with it for the apps and people its administrators
allow. This file explains how to report a vulnerability and what counts as one. The threat model
behind it, with Bancwr's assets, trust boundaries, controls and known limits, is
[docs/threat-model.md](docs/threat-model.md).

## Reporting a vulnerability

**Do not open a public issue, discussion, or pull request for a suspected vulnerability.**

Report it privately:

1. **Preferred:** use GitHub private vulnerability reporting. Open the repository's
   [Security tab](https://github.com/diogel-io/bancwr/security) and choose **Report a
   vulnerability**.
2. **Alternative:** email support@threenine.co.uk with "Bancwr security" in the subject.

Please include:

- the Bancwr version or image tag (and its digest, if you have it), and how it is deployed;
- the steps to reproduce, and what you expected to happen instead;
- the impact: what an attacker gains and what they need first;
- a proof of concept if you have one. Never use a real private key in a report; generate a
  throwaway key for the bunker and for any member.

What to expect:

| Step | Target |
|------|--------|
| Acknowledgement | Within 3 working days |
| Triage | Within 10 working days: we confirm or decline, and agree a severity |
| Fix and release | As fast as the severity needs; we keep you informed until it ships |
| Disclosure | A GitHub security advisory when the fix is released, by default within 90 days of the report |

We credit reporters in the advisory unless you ask us not to. Please give us the chance to release a
fix before you disclose publicly.

## Supported versions

Bancwr has not been released yet: until `v0.1.0`, fixes land on `master` and reach the `edge`
images. From the first release on, only the newest release receives security fixes, as a new
release; see [docs/releasing.md](docs/releasing.md).

| Version | Supported |
|---------|-----------|
| Newest release (from `v0.1.0`) | Yes |
| `master` and `edge`, before the first release | Yes |
| Any earlier release | No |

## Scope

In scope: anything that defeats a Bancwr control in the threat model, for example

- exposure of the bunker's key, the proxy secret, the session password, or a connection token's
  secret;
- the bunker signing an event without a connection token an administrator issued, or of a kind
  the connection was not granted;
- signing in as a key you do not hold, or keeping a session after signing out;
- calling the bunker's API without the frontend, or as an identity other than your own;
- doing more than your role allows, or keeping access after being removed or demoted;
- secrets reaching logs, error messages, or the network;
- a flaw in the build or release pipeline that could ship altered images.

Out of scope:

- an attacker who already controls the host, its environment, or the bunker's data directory
  (the threat model, section 5, "The host is trusted");
- the known limits listed in the threat model, section 5, unless you show they are worse than
  stated;
- the operator's TLS reverse proxy, or a deployment that ignores the README, such as publishing
  the bunker's port;
- vulnerabilities in relays or Nostr apps themselves, rather than in how Bancwr uses them.

If you are unsure, report it anyway.
