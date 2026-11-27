// Departments in the database (#147, departments ADR): a grant's role, administrator only
// identities, who may use and who leads an identity, and what each change records.
use bunker::audit::action;
use bunker::db::{AdminOnlyOutcome, Database, GrantOutcome, NewIdentity};
use bunker::registry::{DepartmentRole, Role};
use chrono::Utc;
use nostr::prelude::*;

fn hex() -> String {
    Keys::generate().public_key().to_hex()
}

/// A bunker with its primary, an administrator and a signer.
fn bunker() -> (Database, String, String) {
    let db = Database::new(":memory:").unwrap();
    db.ensure_primary_identity(&hex(), Utc::now()).unwrap();
    let (admin, signer) = (hex(), hex());
    db.add_team_member("Ann", &admin, Role::Administrator).unwrap();
    db.add_team_member("Sam", &signer, Role::Signer).unwrap();
    (db, admin, signer)
}

fn add(db: &Database, label: &str) -> String {
    let key = hex();
    db.add_identity(&key, label, "line", Utc::now()).unwrap().unwrap();
    key
}

fn actions(db: &Database) -> Vec<String> {
    db.audit_events().unwrap().into_iter().map(|e| e.action).collect()
}

#[test]
fn a_member_of_two_departments_may_use_both_and_no_other() {
    let (db, admin, signer) = bunker();
    let (sales, support, engineering) = (add(&db, "Sales"), add(&db, "Support"), add(&db, "Engineering"));
    db.remove_identity_grant(None, &signer, None, Utc::now()).unwrap();
    db.grant_identity(Some(&sales), &signer, None, Utc::now()).unwrap();
    db.grant_identity(Some(&support), &signer, None, Utc::now()).unwrap();

    assert!(db.may_use_identity(&signer, Some(&sales)).unwrap());
    assert!(db.may_use_identity(&signer, Some(&support)).unwrap());
    assert!(!db.may_use_identity(&signer, Some(&engineering)).unwrap());
    assert!(!db.may_use_identity(&signer, None).unwrap());
    // Administrators use every identity without a grant, and lead none without one.
    assert!(db.may_use_identity(&admin, Some(&engineering)).unwrap());
    assert!(!db.may_lead(&admin, Some(&engineering)).unwrap());
}

#[test]
fn a_lead_leads_only_the_departments_they_hold_a_lead_grant_on() {
    let (db, _, signer) = bunker();
    let (sales, support) = (add(&db, "Sales"), add(&db, "Support"));
    assert_eq!(db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Lead, Some("actor"), Utc::now()).unwrap(), GrantOutcome::Granted);
    db.grant_identity(Some(&support), &signer, None, Utc::now()).unwrap();

    assert!(db.may_lead(&signer, Some(&sales)).unwrap());
    assert!(!db.may_lead(&signer, Some(&support)).unwrap(), "a member of Support, not its lead");
    assert_eq!(db.led_identities(&signer).unwrap(), vec![sales.clone()]);
    let events = db.audit_events().unwrap();
    let appointed = events.iter().find(|e| e.action == action::DEPARTMENT_LEAD_APPOINTED).expect("recorded");
    assert_eq!((appointed.member_pubkey.as_deref(), appointed.identity_pubkey.as_deref()), (Some(signer.as_str()), Some(sales.as_str())));
    assert_eq!(appointed.actor_pubkey.as_deref(), Some("actor"));
}

#[test]
fn making_a_lead_a_member_keeps_their_access_and_records_it() {
    let (db, _, signer) = bunker();
    let sales = add(&db, "Sales");
    db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Lead, None, Utc::now()).unwrap();
    assert_eq!(db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Lead, None, Utc::now()).unwrap(), GrantOutcome::Unchanged);
    assert_eq!(db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Member, None, Utc::now()).unwrap(), GrantOutcome::RoleChanged);

    assert!(!db.may_lead(&signer, Some(&sales)).unwrap());
    assert!(db.may_use_identity(&signer, Some(&sales)).unwrap(), "still a member");
    let removed = db.audit_events().unwrap().into_iter().find(|e| e.action == action::DEPARTMENT_LEAD_REMOVED).expect("recorded");
    assert_eq!(removed.detail.as_deref(), Some("from=lead;to=member"));
}

#[test]
fn an_unrecognised_grant_role_grants_nothing() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap().to_string();
    let db = Database::new(&path).unwrap();
    db.ensure_primary_identity(&hex(), Utc::now()).unwrap();
    let signer = hex();
    db.add_team_member("Sam", &signer, Role::Signer).unwrap();
    let sales = add(&db, "Sales");
    db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Lead, None, Utc::now()).unwrap();
    // A value no version of Bancwr writes (T4).
    rusqlite::Connection::open(&path).unwrap().execute("UPDATE identity_access SET role = 'boss' WHERE member_pubkey = ?1", [&signer]).unwrap();

    assert!(!db.may_lead(&signer, Some(&sales)).unwrap());
    assert!(!db.may_use_identity(&signer, Some(&sales)).unwrap());
    assert!(db.led_identities(&signer).unwrap().is_empty());
    assert_eq!(db.find_grant(Some(&sales), &signer).unwrap().unwrap().role, None);
}

#[test]
fn the_first_lead_in_a_single_identity_bunker_first_grants_the_primary_to_everyone() {
    let (db, admin, signer) = bunker();
    let other = hex();
    db.add_team_member("Pam", &other, Role::Signer).unwrap();
    assert!(!db.grants_matter().unwrap());

    db.grant_identity_as(None, &other, DepartmentRole::Lead, None, Utc::now()).unwrap();

    assert!(db.grants_matter().unwrap());
    assert!(db.may_use_identity(&signer, None).unwrap(), "nobody loses the primary");
    assert!(db.may_lead(&other, None).unwrap());
    let members = db.identity_grants(None).unwrap();
    assert_eq!(members.len(), 3, "everyone who may sign holds the primary: {:?}", members);
    assert!(members.iter().any(|g| g.member_pubkey == admin && g.role == Some(DepartmentRole::Member)));
    assert!(members.iter().any(|g| g.member_pubkey == signer && g.role == Some(DepartmentRole::Member)));
}

#[test]
fn an_administrator_only_identity_refuses_grants_and_leads() {
    let (db, admin, signer) = bunker();
    let root = hex();
    db.add_identity_with(&root, "Company root", "line", None, NewIdentity { admin_only: true, lead: None }, Utc::now()).unwrap().unwrap();
    assert!(db.find_identity(&root).unwrap().unwrap().admin_only);

    assert_eq!(db.grant_identity_as(Some(&root), &signer, DepartmentRole::Member, None, Utc::now()).unwrap(), GrantOutcome::AdminOnly);
    assert_eq!(db.grant_identity_as(Some(&root), &signer, DepartmentRole::Lead, None, Utc::now()).unwrap(), GrantOutcome::AdminOnly);
    assert!(!db.may_use_identity(&signer, Some(&root)).unwrap());
    assert!(db.may_use_identity(&admin, Some(&root)).unwrap());
    assert!(actions(&db).contains(&action::IDENTITY_ADMIN_ONLY_CHANGED.to_string()));
}

#[test]
fn marking_an_identity_administrator_only_is_refused_while_it_has_members() {
    let (db, _, signer) = bunker();
    let sales = add(&db, "Sales");
    db.grant_identity(Some(&sales), &signer, None, Utc::now()).unwrap();
    assert_eq!(db.set_identity_admin_only(Some(&sales), true, None, Utc::now()).unwrap(), AdminOnlyOutcome::HasMembers);
    assert!(db.may_use_identity(&signer, Some(&sales)).unwrap(), "nothing changed");

    db.remove_identity_grant(Some(&sales), &signer, None, Utc::now()).unwrap();
    assert_eq!(db.set_identity_admin_only(Some(&sales), true, Some("ann"), Utc::now()).unwrap(), AdminOnlyOutcome::Changed);
    assert_eq!(db.set_identity_admin_only(Some(&sales), true, None, Utc::now()).unwrap(), AdminOnlyOutcome::Unchanged);
    assert_eq!(db.set_identity_admin_only(Some(&sales), false, None, Utc::now()).unwrap(), AdminOnlyOutcome::Changed);
    let changes: Vec<_> = db.audit_events().unwrap().into_iter().filter(|e| e.action == action::IDENTITY_ADMIN_ONLY_CHANGED).collect();
    assert_eq!(changes.iter().map(|e| e.detail.clone().unwrap()).collect::<Vec<_>>(), ["admin_only=true", "admin_only=false"]);
}

#[test]
fn an_administrator_only_primary_in_a_single_identity_bunker_is_for_administrators_alone() {
    let db = Database::new(":memory:").unwrap();
    db.ensure_primary_identity(&hex(), Utc::now()).unwrap();
    let admin = hex();
    db.add_team_member("Ann", &admin, Role::Administrator).unwrap();
    assert_eq!(db.set_identity_admin_only(None, true, None, Utc::now()).unwrap(), AdminOnlyOutcome::Changed);

    // A signer added later may not use it, and adding an identity beside it grants it to nobody.
    let signer = hex();
    db.add_team_member("Sam", &signer, Role::Signer).unwrap();
    assert!(!db.may_use_identity(&signer, None).unwrap());
    assert!(db.may_use_identity(&admin, None).unwrap());
    add(&db, "Sales");
    assert!(db.identity_grants(None).unwrap().is_empty());
}

#[test]
fn marking_a_shared_primary_is_refused_while_signers_use_it_without_grants() {
    let (db, _, _) = bunker();
    assert_eq!(db.set_identity_admin_only(None, true, None, Utc::now()).unwrap(), AdminOnlyOutcome::HasMembers);
}

#[test]
fn a_lead_who_creates_an_identity_leads_its_new_department() {
    let (db, _, signer) = bunker();
    let sales = add(&db, "Sales");
    db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Lead, None, Utc::now()).unwrap();
    let marketing = hex();
    db.add_identity_with(&marketing, "Marketing", "line", Some(&signer), NewIdentity { admin_only: false, lead: Some(&signer) }, Utc::now())
        .unwrap()
        .unwrap();
    assert!(db.may_lead(&signer, Some(&marketing)).unwrap());
    let events = db.audit_events().unwrap();
    let appointed: Vec<_> = events.iter().filter(|e| e.action == action::DEPARTMENT_LEAD_APPOINTED && e.identity_pubkey.as_deref() == Some(&marketing)).collect();
    assert_eq!(appointed.len(), 1);
    assert_eq!(appointed[0].actor_pubkey.as_deref(), Some(signer.as_str()));
}

#[test]
fn a_member_registered_into_departments_is_granted_them_in_one_go() {
    let (db, _, lead) = bunker();
    let sales = add(&db, "Sales");
    let new = hex();
    db.add_team_member_with("Paul", &new, Role::Signer, Some(&lead), &[Some(sales.clone())]).unwrap();
    assert!(db.may_use_identity(&new, Some(&sales)).unwrap());
    assert!(!db.may_use_identity(&new, None).unwrap());
    let added = db.audit_events().unwrap().into_iter().find(|e| e.action == action::MEMBER_ADDED && e.member_pubkey.as_deref() == Some(&new)).unwrap();
    assert_eq!(added.actor_pubkey.as_deref(), Some(lead.as_str()));
    assert_eq!(added.detail, Some(format!("role=signer;departments={}", sales)));
}
