// Departments end to end (#147, departments ADR), through the guarded API: the threenine.io
// example, with Sales (Pam leads it, Peter is a member), Support (James), Sam in both, and the
// company root as an administrator only identity. A lead manages their own department's members,
// connections, approvals and identity, and is refused on every other department and on the root.
#[path = "../common/mod.rs"]
mod common;

use bunker::approvals_api::ApprovalResponse;
use bunker::audit::action;
use bunker::connections_api::{ConnectionResponse, IssueTokenResponse, TokenResponse};
use bunker::db::AdminOnlyOutcome;
use bunker::identities_api::IdentityResponse;
use bunker::approvals::Scheme;
use bunker::nip46::{Nip46Outcome, Nip46Request};
use bunker::registry::{DepartmentRole, Role};
use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

struct Company {
    app: common::TestApp,
    sales: String,
    support: String,
    root: String,
    pam: String,
    peter: String,
    james: String,
    sam: String,
}

fn hex(keys: &Keys) -> String {
    keys.public_key().to_hex()
}

async fn company() -> Company {
    let (sales_keys, support_keys, root) = (Keys::generate(), Keys::generate(), Keys::generate());
    let identities = [("Sales", sales_keys.clone()), ("Support", support_keys.clone()), ("Company root", root.clone())];
    let app = common::spawn_with_identities(true, Some(vec!["wss://relay.example.com".to_string()]), &identities).await;
    let (sales, support) = (hex(&sales_keys), hex(&support_keys));
    assert_eq!(app.db.set_identity_admin_only(Some(&hex(&root)), true, None, chrono::Utc::now()).unwrap(), AdminOnlyOutcome::Changed);
    let mut people = Vec::new();
    for name in ["Pam", "Peter", "James", "Sam"] {
        let keys = Keys::generate();
        app.db.add_team_member(name, &hex(&keys), Role::Signer).unwrap();
        people.push(hex(&keys));
    }
    let (pam, peter, james, sam) = (people[0].clone(), people[1].clone(), people[2].clone(), people[3].clone());
    let now = chrono::Utc::now();
    app.db.grant_identity_as(Some(&sales), &pam, DepartmentRole::Lead, None, now).unwrap();
    app.db.grant_identity(Some(&sales), &peter, None, now).unwrap();
    app.db.grant_identity(Some(&support), &james, None, now).unwrap();
    app.db.grant_identity(Some(&sales), &sam, None, now).unwrap();
    app.db.grant_identity(Some(&support), &sam, None, now).unwrap();
    Company { app, sales, support, root: hex(&root), pam, peter, james, sam }
}

async fn send(app: &common::TestApp, method: Method, path: &str, who: &str, body: Option<Value>) -> reqwest::Response {
    let request = app.signed(method, path, who);
    match body {
        Some(body) => request.json(&body),
        None => request,
    }
    .send()
    .await
    .unwrap()
}

async fn error(res: reqwest::Response) -> (StatusCode, Value) {
    let status = res.status();
    (status, res.json::<Value>().await.unwrap_or(Value::Null)["error"].clone())
}

fn token_body(identity: &str, for_pubkey: &str) -> Value {
    json!({ "label": "Bot", "kinds": [1, 7], "approval_kinds": [1], "identity": identity, "for_pubkey": for_pubkey })
}

#[tokio::test]
async fn a_member_in_two_departments_may_sign_as_both_and_no_other() {
    let c = company().await;
    let listed: Vec<IdentityResponse> = send(&c.app, Method::GET, "/api/bunker/identities", &c.sam, None).await.json().await.unwrap();
    let mut keys: Vec<&str> = listed.iter().map(|i| i.pubkey.as_str()).collect();
    keys.sort();
    let mut expected = vec![c.sales.as_str(), c.support.as_str()];
    expected.sort();
    assert_eq!(keys, expected, "Sales and Support, not the primary or the root");
    assert!(listed.iter().all(|i| i.my_role == Some(DepartmentRole::Member) && i.members.is_none()));

    for (identity, allowed) in [(&c.sales, true), (&c.support, true), (&c.root, false)] {
        let res = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.sam, Some(json!({ "label": "x", "kinds": [1], "identity": identity }))).await;
        assert_eq!(res.status() == StatusCode::OK, allowed, "{} -> {}", identity, res.status());
    }
}

#[tokio::test]
async fn a_lead_reads_the_team_and_sees_their_department_with_its_members_and_leads() {
    let c = company().await;
    assert_eq!(send(&c.app, Method::GET, "/api/bunker/team", &c.pam, None).await.status(), StatusCode::OK);
    assert_eq!(error(send(&c.app, Method::GET, "/api/bunker/team", &c.peter, None).await).await, (StatusCode::FORBIDDEN, json!("forbidden")));

    let listed: Vec<IdentityResponse> = send(&c.app, Method::GET, "/api/bunker/identities", &c.pam, None).await.json().await.unwrap();
    assert_eq!(listed.len(), 1, "Pam is in Sales only");
    let sales = &listed[0];
    assert_eq!(sales.my_role, Some(DepartmentRole::Lead));
    assert_eq!(sales.leads.as_deref(), Some(&[c.pam.clone()][..]));
    let mut members = sales.members.clone().unwrap();
    members.sort();
    let mut expected = vec![c.pam.clone(), c.peter.clone(), c.sam.clone()];
    expected.sort();
    assert_eq!(members, expected);

    // An administrator sees the root marked, with no members.
    let all: Vec<IdentityResponse> = send(&c.app, Method::GET, "/api/bunker/identities", &hex(&c.app.admin), None).await.json().await.unwrap();
    let root = all.iter().find(|i| i.pubkey == c.root).unwrap();
    assert!(root.admin_only);
    assert_eq!(root.members.as_deref(), Some(&[][..]));
}

#[tokio::test]
async fn a_lead_adds_and_removes_members_of_their_department_only() {
    let c = company().await;
    let path = |identity: &str, member: &str| format!("/api/bunker/identities/{}/members/{}", identity, member);

    // James joins Sales, as a member.
    assert_eq!(send(&c.app, Method::PUT, &path(&c.sales, &c.james), &c.pam, None).await.status(), StatusCode::OK);
    assert!(c.app.db.may_use_identity(&c.james, Some(&c.sales)).unwrap());
    let granted = c.app.db.audit_events().unwrap().into_iter().filter(|e| e.action == action::IDENTITY_ACCESS_GRANTED).last().unwrap();
    assert_eq!((granted.actor_pubkey.as_deref(), granted.member_pubkey.as_deref()), (Some(c.pam.as_str()), Some(c.james.as_str())));

    // Never into Support, never as a lead, and never the root.
    assert_eq!(error(send(&c.app, Method::PUT, &path(&c.support, &c.peter), &c.pam, None).await).await.0, StatusCode::FORBIDDEN);
    assert_eq!(error(send(&c.app, Method::PUT, &path(&c.sales, &c.peter), &c.pam, Some(json!({ "role": "lead" }))).await).await.0, StatusCode::FORBIDDEN);
    assert_eq!(error(send(&c.app, Method::PUT, &path(&c.root, &c.peter), &c.pam, None).await).await.0, StatusCode::FORBIDDEN);
    assert!(!c.app.db.may_use_identity(&c.peter, Some(&c.support)).unwrap());

    // Removing a member, but not from Support, and not another lead.
    assert_eq!(send(&c.app, Method::DELETE, &path(&c.sales, &c.james), &c.pam, None).await.status(), StatusCode::OK);
    assert_eq!(error(send(&c.app, Method::DELETE, &path(&c.support, &c.james), &c.pam, None).await).await.0, StatusCode::FORBIDDEN);
    c.app.db.grant_identity_as(Some(&c.sales), &c.peter, DepartmentRole::Lead, None, chrono::Utc::now()).unwrap();
    assert_eq!(error(send(&c.app, Method::DELETE, &path(&c.sales, &c.peter), &c.pam, None).await).await, (StatusCode::FORBIDDEN, json!("forbidden")));
    assert_eq!(error(send(&c.app, Method::PUT, &path(&c.sales, &c.peter), &c.pam, Some(json!({ "role": "member" }))).await).await.0, StatusCode::FORBIDDEN);
    // A lead may step down, or leave.
    assert_eq!(send(&c.app, Method::PUT, &path(&c.sales, &c.pam), &c.pam, Some(json!({ "role": "member" }))).await.status(), StatusCode::OK);
    assert!(!c.app.db.may_lead(&c.pam, Some(&c.sales)).unwrap());
    assert!(c.app.db.may_use_identity(&c.pam, Some(&c.sales)).unwrap());
}

#[tokio::test]
async fn only_an_administrator_appoints_leads_and_marks_an_identity_administrator_only() {
    let c = company().await;
    let admin = hex(&c.app.admin);
    let lead = send(&c.app, Method::PUT, &format!("/api/bunker/identities/{}/members/{}", c.support, c.james), &admin, Some(json!({ "role": "lead" }))).await;
    assert_eq!(lead.status(), StatusCode::OK);
    assert!(c.app.db.may_lead(&c.james, Some(&c.support)).unwrap());
    let bad = send(&c.app, Method::PUT, &format!("/api/bunker/identities/{}/members/{}", c.support, c.james), &admin, Some(json!({ "role": "boss" }))).await;
    assert_eq!(error(bad).await, (StatusCode::BAD_REQUEST, json!("invalid_role")));

    // The root refuses grants; Support, with members, can't be marked.
    let refused = send(&c.app, Method::PUT, &format!("/api/bunker/identities/{}/members/{}", c.root, c.peter), &admin, None).await;
    assert_eq!(error(refused).await, (StatusCode::CONFLICT, json!("admin_only")));
    let marked = send(&c.app, Method::PATCH, &format!("/api/bunker/identities/{}", c.support), &admin, Some(json!({ "admin_only": true }))).await;
    assert_eq!(error(marked).await, (StatusCode::CONFLICT, json!("has_members")));
    // A lead renames their department, but never marks it.
    let renamed = send(&c.app, Method::PATCH, &format!("/api/bunker/identities/{}", c.sales), &c.pam, Some(json!({ "label": "Sales EMEA" }))).await;
    assert_eq!(renamed.status(), StatusCode::OK);
    assert_eq!(c.app.db.find_identity(&c.sales).unwrap().unwrap().label, "Sales EMEA");
    let lead_marks = send(&c.app, Method::PATCH, &format!("/api/bunker/identities/{}", c.sales), &c.pam, Some(json!({ "admin_only": true }))).await;
    assert_eq!(lead_marks.status(), StatusCode::FORBIDDEN);
    let other = send(&c.app, Method::PATCH, &format!("/api/bunker/identities/{}", c.support), &c.pam, Some(json!({ "label": "Mine now" }))).await;
    assert_eq!(other.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_lead_registers_signers_into_their_departments_only() {
    let c = company().await;
    let new = |role: &str, identities: Value| json!({ "name": "Paul", "pubkey": hex(&Keys::generate()), "role": role, "identities": identities });

    let added = send(&c.app, Method::POST, "/api/bunker/team", &c.pam, Some(new("signer", json!([c.sales])))).await;
    assert_eq!(added.status(), StatusCode::OK);
    let paul = c.app.db.get_team_members().unwrap().into_iter().find(|m| m.name == "Paul").unwrap();
    assert!(c.app.db.may_use_identity(&paul.pubkey, Some(&c.sales)).unwrap());
    assert!(!c.app.db.may_use_identity(&paul.pubkey, None).unwrap());
    let event = c.app.db.audit_events().unwrap().into_iter().find(|e| e.action == action::MEMBER_ADDED && e.member_pubkey.as_deref() == Some(&paul.pubkey)).unwrap();
    assert_eq!(event.actor_pubkey.as_deref(), Some(c.pam.as_str()));
    assert_eq!(event.detail, Some(format!("role=signer;departments={}", c.sales)));

    for body in [new("signer", json!([c.support])), new("administrator", json!([c.sales])), new("signer", json!([])), new("signer", json!([c.sales, c.support]))] {
        let res = send(&c.app, Method::POST, "/api/bunker/team", &c.pam, Some(body.clone())).await;
        assert_eq!(error(res).await, (StatusCode::FORBIDDEN, json!("not_your_department")), "{}", body);
    }
    // An identity's key is refused for a lead as for an administrator (T2).
    let identity_key = json!({ "name": "Sneaky", "pubkey": c.support, "role": "signer", "identities": [c.sales] });
    assert_eq!(send(&c.app, Method::POST, "/api/bunker/team", &c.pam, Some(identity_key)).await.status(), StatusCode::BAD_REQUEST);
    // A plain member registers nobody.
    assert_eq!(error(send(&c.app, Method::POST, "/api/bunker/team", &c.peter, Some(new("signer", json!([c.sales])))).await).await, (StatusCode::FORBIDDEN, json!("forbidden")));
}

#[tokio::test]
async fn a_lead_issues_lists_and_revokes_their_departments_connections_only() {
    let c = company().await;
    // For Peter, as Sales, with approval kinds.
    let res = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.pam, Some(token_body(&c.sales, &c.peter))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let sales_token: IssueTokenResponse = res.json().await.unwrap();
    assert_eq!(sales_token.approval_kinds, vec![1]);

    // Never as Support, never for someone outside Sales, never as the root.
    let support = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.pam, Some(token_body(&c.support, &c.james))).await;
    assert_eq!(error(support).await, (StatusCode::FORBIDDEN, json!("approval_admin_only")));
    let support_plain = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.pam, Some(json!({ "label": "x", "kinds": [1], "identity": c.support, "for_pubkey": c.james }))).await;
    assert_eq!(error(support_plain).await, (StatusCode::FORBIDDEN, json!("forbidden_member")));
    let outsider = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.pam, Some(token_body(&c.sales, &c.james))).await;
    assert_eq!(error(outsider).await, (StatusCode::FORBIDDEN, json!("not_your_department")));
    let root = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.pam, Some(json!({ "label": "x", "kinds": [1], "identity": c.root }))).await;
    assert_eq!(error(root).await, (StatusCode::BAD_REQUEST, json!("no_identity_access")));
    // Peter, a member, still only for himself and without approval kinds.
    let member = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.peter, Some(token_body(&c.sales, &c.peter))).await;
    assert_eq!(error(member).await, (StatusCode::FORBIDDEN, json!("approval_admin_only")));

    // An administrator's Support token for James is not Pam's to see or revoke.
    let admin = hex(&c.app.admin);
    let james_token: IssueTokenResponse =
        send(&c.app, Method::POST, "/api/bunker/connections/tokens", &admin, Some(json!({ "label": "x", "kinds": [1], "identity": c.support, "for_pubkey": c.james }))).await.json().await.unwrap();
    let listed: Vec<TokenResponse> = send(&c.app, Method::GET, "/api/bunker/connections/tokens", &c.pam, None).await.json().await.unwrap();
    assert_eq!(listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), [sales_token.id.as_str()]);
    let revoke_other = send(&c.app, Method::DELETE, &format!("/api/bunker/connections/tokens/{}", james_token.id), &c.pam, None).await;
    assert_eq!(revoke_other.status(), StatusCode::NOT_FOUND);
    // Peter sees his own token, issued by Pam.
    let peters: Vec<TokenResponse> = send(&c.app, Method::GET, "/api/bunker/connections/tokens", &c.peter, None).await.json().await.unwrap();
    assert_eq!(peters.len(), 1);
    assert_eq!(send(&c.app, Method::DELETE, &format!("/api/bunker/connections/tokens/{}", sales_token.id), &c.pam, None).await.status(), StatusCode::OK);
}

/// Connects an app with `token`'s secret, addressed to `identity`, through the handler.
async fn connect(c: &Company, token: &IssueTokenResponse, identity: &str) -> Keys {
    let secret = token.uri.split("secret=").nth(1).unwrap().to_string();
    let client = Keys::generate();
    let outcome = c
        .app
        .state
        .nip46_handler
        .handle_for(Nip46Request { id: "c".into(), method: "connect".into(), params: vec![identity.to_string(), secret] }, client.public_key(), Scheme::Nip44, Some(identity.to_string()))
        .await;
    match outcome {
        Nip46Outcome::Reply(response) => assert_eq!(response.result.as_deref(), Some("ack")),
        Nip46Outcome::Held { .. } => panic!("connect is never held"),
    }
    client
}

/// Has `client` ask to sign a kind 1 note as `identity`, which its token holds for approval.
async fn hold(c: &Company, client: &Keys, identity: &str) {
    let unsigned = EventBuilder::text_note("launch day").build(PublicKey::from_hex(identity).unwrap());
    let outcome = c
        .app
        .state
        .nip46_handler
        .handle_for(
            Nip46Request { id: "s".into(), method: "sign_event".into(), params: vec![serde_json::to_string(&unsigned).unwrap()] },
            client.public_key(),
            Scheme::Nip44,
            Some(identity.to_string()),
        )
        .await;
    assert!(matches!(outcome, Nip46Outcome::Held { .. }), "held for approval");
}

#[tokio::test]
async fn a_lead_decides_their_departments_held_requests_their_own_included_and_no_others() {
    let c = company().await;
    let admin = hex(&c.app.admin);
    let issue = |identity: &str, member: &str| token_body(identity, member);
    let peters: IssueTokenResponse = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.pam, Some(issue(&c.sales, &c.peter))).await.json().await.unwrap();
    let pams: IssueTokenResponse = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &c.pam, Some(issue(&c.sales, &c.pam))).await.json().await.unwrap();
    let james: IssueTokenResponse = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &admin, Some(issue(&c.support, &c.james))).await.json().await.unwrap();
    let (peter_app, pam_app, james_app) = (connect(&c, &peters, &c.sales).await, connect(&c, &pams, &c.sales).await, connect(&c, &james, &c.support).await);
    hold(&c, &peter_app, &c.sales).await;
    hold(&c, &pam_app, &c.sales).await;
    hold(&c, &james_app, &c.support).await;

    let listed: Vec<ApprovalResponse> = send(&c.app, Method::GET, "/api/bunker/approvals", &c.pam, None).await.json().await.unwrap();
    assert_eq!(listed.len(), 2, "Sales's two, not Support's");
    assert!(listed.iter().all(|r| r.identity_pubkey.as_deref() == Some(c.sales.as_str())));
    let all: Vec<ApprovalResponse> = send(&c.app, Method::GET, "/api/bunker/approvals", &admin, None).await.json().await.unwrap();
    let support_request = all.iter().find(|r| r.identity_pubkey.as_deref() == Some(c.support.as_str())).unwrap();

    // Support's request is not Pam's: as if it did not exist.
    let refused = send(&c.app, Method::POST, &format!("/api/bunker/approvals/{}/approve", support_request.id), &c.pam, Some(json!({ "event_id": support_request.event_id }))).await;
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);
    let refused = send(&c.app, Method::POST, &format!("/api/bunker/approvals/{}/reject", support_request.id), &c.pam, Some(json!({}))).await;
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);
    // Peter, a member, decides nothing, not even his own.
    let peters_request = listed.iter().find(|r| r.member_pubkey == c.peter).unwrap();
    let own = send(&c.app, Method::POST, &format!("/api/bunker/approvals/{}/approve", peters_request.id), &c.peter, Some(json!({ "event_id": peters_request.event_id }))).await;
    assert_eq!(own.status(), StatusCode::FORBIDDEN);

    // Pam approves Peter's, and her own, which is recorded as hers.
    for request in &listed {
        let res = send(&c.app, Method::POST, &format!("/api/bunker/approvals/{}/approve", request.id), &c.pam, Some(json!({ "event_id": request.event_id }))).await;
        assert_eq!(res.status(), StatusCode::OK);
    }
    let approved: Vec<_> = c.app.db.audit_events().unwrap().into_iter().filter(|e| e.action == action::APPROVAL_APPROVED).collect();
    assert_eq!(approved.len(), 2);
    assert!(approved.iter().all(|e| e.actor_pubkey.as_deref() == Some(c.pam.as_str())));
    let mine = approved.iter().find(|e| e.member_pubkey.as_deref() == Some(c.pam.as_str())).unwrap();
    assert!(mine.detail.as_deref().unwrap().contains(";self=true"), "{:?}", mine.detail);
    let peters_event = approved.iter().find(|e| e.member_pubkey.as_deref() == Some(c.peter.as_str())).unwrap();
    assert!(!peters_event.detail.as_deref().unwrap().contains("self=true"));
    assert_eq!(c.app.db.signature_count().unwrap(), 2);
}

#[tokio::test]
async fn removing_a_member_from_a_department_revokes_only_their_connections_for_it() {
    let c = company().await;
    let admin = hex(&c.app.admin);
    let plain = |identity: &str, member: &str| json!({ "label": "x", "kinds": [1], "identity": identity, "for_pubkey": member });
    let sales_token: IssueTokenResponse = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &admin, Some(plain(&c.sales, &c.sam))).await.json().await.unwrap();
    let support_token: IssueTokenResponse = send(&c.app, Method::POST, "/api/bunker/connections/tokens", &admin, Some(plain(&c.support, &c.sam))).await.json().await.unwrap();
    connect(&c, &sales_token, &c.sales).await;
    connect(&c, &support_token, &c.support).await;

    let res = send(&c.app, Method::DELETE, &format!("/api/bunker/identities/{}/members/{}", c.sales, c.sam), &c.pam, None).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.json::<Value>().await.unwrap()["connections_revoked"], 1);

    let listed: Vec<ConnectionResponse> = send(&c.app, Method::GET, "/api/bunker/connections", &c.sam, None).await.json().await.unwrap();
    let state = |identity: &str| listed.iter().find(|x| x.identity_pubkey.as_deref() == Some(identity)).unwrap().revoked_reason.clone();
    assert_eq!(state(&c.sales).as_deref(), Some("access_removed"));
    assert_eq!(state(&c.support), None, "Support's connection is untouched");
}

#[tokio::test]
async fn a_lead_creates_a_department_they_lead_and_retires_only_their_own() {
    let c = company().await;
    let res = send(&c.app, Method::POST, "/api/bunker/identities", &c.pam, Some(json!({ "label": "Marketing" }))).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let marketing: IdentityResponse = res.json().await.unwrap();
    assert_eq!(marketing.my_role, Some(DepartmentRole::Lead));
    assert_eq!(marketing.leads.as_deref(), Some(&[c.pam.clone()][..]));
    assert!(c.app.db.may_lead(&c.pam, Some(&marketing.pubkey)).unwrap());
    // A member creates nothing.
    assert_eq!(send(&c.app, Method::POST, "/api/bunker/identities", &c.peter, Some(json!({ "label": "Mine" }))).await.status(), StatusCode::FORBIDDEN);

    // She retires Marketing, never Support, the root or the primary.
    for identity in [&c.support, &c.root] {
        assert_eq!(send(&c.app, Method::DELETE, &format!("/api/bunker/identities/{}", identity), &c.pam, None).await.status(), StatusCode::FORBIDDEN, "{}", identity);
    }
    assert_eq!(send(&c.app, Method::DELETE, &format!("/api/bunker/identities/{}", marketing.pubkey), &c.pam, None).await.status(), StatusCode::OK);
    assert!(c.app.db.find_identity(&marketing.pubkey).unwrap().unwrap().retired_at.is_some());
}

#[tokio::test]
async fn a_lead_signs_their_departments_profile_and_nobody_elses() {
    let c = company().await;
    let profile = json!({ "kind": 0, "created_at": chrono::Utc::now().timestamp(), "tags": [], "content": "{\"name\":\"Sales\"}" });
    assert_eq!(send(&c.app, Method::POST, &format!("/api/bunker/identities/{}/sign", c.sales), &c.pam, Some(profile.clone())).await.status(), StatusCode::OK);
    for identity in [&c.support, &c.root] {
        assert_eq!(send(&c.app, Method::POST, &format!("/api/bunker/identities/{}/sign", identity), &c.pam, Some(profile.clone())).await.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(send(&c.app, Method::POST, &format!("/api/bunker/identities/{}/sign", c.sales), &c.peter, Some(profile)).await.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_lead_reads_no_logs_metrics_relays_or_configuration_and_changes_no_roles() {
    let c = company().await;
    for path in ["/api/bunker/logs", "/api/bunker/metrics", "/api/bunker/relays", "/api/bunker/config"] {
        assert_eq!(send(&c.app, Method::GET, path, &c.pam, None).await.status(), StatusCode::FORBIDDEN, "{}", path);
    }
    let peter = c.app.db.find_member_by_pubkey(&c.peter).unwrap().unwrap().id;
    assert_eq!(send(&c.app, Method::PATCH, &format!("/api/bunker/team/{}", peter), &c.pam, Some(json!({ "role": "administrator" }))).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(send(&c.app, Method::DELETE, &format!("/api/bunker/team/{}", peter), &c.pam, None).await.status(), StatusCode::FORBIDDEN);
}
