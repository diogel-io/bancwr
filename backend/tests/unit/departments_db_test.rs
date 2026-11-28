// Departments in the database (#147, departments ADR): a grant's role, administrator only
// identities, who may use and who leads an identity, and what each change records.
use bunker::audit::action;
use bunker::db::{AddIdentityRefusal, AddMemberOutcome, AdminOnlyOutcome, Database, Departments, GrantOutcome, NewIdentity, RemoveGrantOutcome, UpdateIdentityOutcome};
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
fn once_a_lead_was_appointed_grants_keep_deciding_after_the_last_lead_steps_down() {
    let (db, _, signer) = bunker();
    let lead = hex();
    db.add_team_member("Pam", &lead, Role::Signer).unwrap();
    db.grant_identity_as(None, &lead, DepartmentRole::Lead, None, Utc::now()).unwrap();
    // The lead takes Sam out of the primary's department, then steps down.
    db.remove_identity_grant(None, &signer, Some(&lead), Utc::now()).unwrap();
    db.grant_identity_as(None, &lead, DepartmentRole::Member, Some(&lead), Utc::now()).unwrap();

    assert!(db.grants_matter().unwrap());
    assert!(!db.may_use_identity(&signer, None).unwrap(), "Sam does not get the primary back");
    assert!(db.may_use_identity(&lead, None).unwrap());
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
    db.add_team_member_with("Paul", &new, Role::Signer, Some(&lead), None, Departments::Named(&[Some(sales.clone())])).unwrap();
    assert!(db.may_use_identity(&new, Some(&sales)).unwrap());
    assert!(!db.may_use_identity(&new, None).unwrap());
    let added = db.audit_events().unwrap().into_iter().find(|e| e.action == action::MEMBER_ADDED && e.member_pubkey.as_deref() == Some(&new)).unwrap();
    assert_eq!(added.actor_pubkey.as_deref(), Some(lead.as_str()));
    assert_eq!(added.detail, Some(format!("role=signer;departments={}", sales)));
}

#[test]
fn the_primary_grants_made_when_grants_start_to_matter_are_each_recorded() {
    let (db, admin, signer) = bunker();
    let lead = hex();
    db.add_team_member("Pam", &lead, Role::Signer).unwrap();
    db.grant_identity_as(None, &lead, DepartmentRole::Lead, Some(&admin), Utc::now()).unwrap();

    let granted: Vec<_> = db.audit_events().unwrap().into_iter().filter(|e| e.action == action::IDENTITY_ACCESS_GRANTED && e.identity_pubkey.is_none()).collect();
    let mut members: Vec<&str> = granted.iter().filter_map(|e| e.member_pubkey.as_deref()).collect();
    members.sort();
    let mut expected = vec![admin.as_str(), signer.as_str(), lead.as_str()];
    expected.sort();
    assert_eq!(members, expected, "one event for each new grant, the lead's included, none twice");
    assert!(granted.iter().all(|e| e.actor_pubkey.is_none()), "each made by the bunker, the lead's included: {:?}", granted);
}

#[test]
fn registering_a_member_into_an_administrator_only_department_stores_nothing() {
    let (db, admin, _) = bunker();
    let root = hex();
    db.add_identity_with(&root, "Company root", "line", None, NewIdentity { admin_only: true, lead: None }, Utc::now()).unwrap().unwrap();
    let new = hex();
    let outcome = db.add_team_member_with("Paul", &new, Role::Signer, Some(&admin), None, Departments::Named(&[Some(root.clone())])).unwrap();
    assert_eq!(outcome, AddMemberOutcome::AdminOnly);
    assert!(db.find_member_by_pubkey(&new).unwrap().is_none(), "the member was not stored either");
    assert!(db.identity_grants(Some(&root)).unwrap().is_empty());
}

#[test]
fn a_member_who_leads_nothing_now_cannot_start_a_department() {
    let (db, _, signer) = bunker();
    let key = hex();
    let refused = db.add_identity_with(&key, "Marketing", "line", Some(&signer), NewIdentity { admin_only: false, lead: Some(&signer) }, Utc::now()).unwrap();
    assert_eq!(refused, Err(AddIdentityRefusal::NotALead));
    assert!(db.find_identity(&key).unwrap().is_none());
}

#[test]
fn a_lead_changes_no_grant_once_they_no_longer_lead_and_never_another_leads() {
    let (db, _, signer) = bunker();
    let sales = add(&db, "Sales");
    let (pam, lee) = (hex(), hex());
    db.add_team_member("Pam", &pam, Role::Signer).unwrap();
    db.add_team_member("Lee", &lee, Role::Signer).unwrap();
    db.grant_identity_as(Some(&sales), &pam, DepartmentRole::Lead, None, Utc::now()).unwrap();
    db.grant_identity_as(Some(&sales), &lee, DepartmentRole::Lead, None, Utc::now()).unwrap();
    db.grant_identity(Some(&sales), &signer, None, Utc::now()).unwrap();

    // Another lead, read in the same transaction as the write.
    let member = DepartmentRole::Member;
    assert_eq!(db.grant_identity_by(Some(&sales), &lee, member, Some(&pam), Some(&pam), Utc::now()).unwrap(), GrantOutcome::AnotherLead);
    assert_eq!(db.remove_identity_grant_by(Some(&sales), &lee, Some(&pam), Some(&pam), Utc::now()).unwrap(), RemoveGrantOutcome::AnotherLead);
    assert!(db.may_lead(&lee, Some(&sales)).unwrap());

    // Pam steps down: she may still make herself a member, and then changes nothing.
    assert_eq!(db.grant_identity_by(Some(&sales), &pam, member, Some(&pam), Some(&pam), Utc::now()).unwrap(), GrantOutcome::RoleChanged);
    assert_eq!(db.remove_identity_grant_by(Some(&sales), &signer, Some(&pam), Some(&pam), Utc::now()).unwrap(), RemoveGrantOutcome::NotALead);
    assert_eq!(db.grant_identity_by(Some(&sales), &signer, member, Some(&pam), Some(&pam), Utc::now()).unwrap(), GrantOutcome::NotALead);
    assert!(db.may_use_identity(&signer, Some(&sales)).unwrap(), "Sam keeps Sales");
}

#[test]
fn a_refused_mark_leaves_the_label_unchanged_too() {
    let (db, _, signer) = bunker();
    let sales = add(&db, "Sales");
    db.grant_identity(Some(&sales), &signer, None, Utc::now()).unwrap();
    let outcome = db.update_identity(&sales, Some(&sales), Some("Sales EMEA"), Some(true), None, None, Utc::now()).unwrap();
    assert_eq!(outcome, UpdateIdentityOutcome::HasMembers);
    assert_eq!(db.find_identity(&sales).unwrap().unwrap().label, "Sales");
    assert!(!actions(&db).contains(&action::IDENTITY_RENAMED.to_string()));

    db.remove_identity_grant(Some(&sales), &signer, None, Utc::now()).unwrap();
    assert_eq!(db.update_identity(&sales, Some(&sales), Some("Sales EMEA"), Some(true), None, None, Utc::now()).unwrap(), UpdateIdentityOutcome::Updated);
    let identity = db.find_identity(&sales).unwrap().unwrap();
    assert!(identity.admin_only && identity.label == "Sales EMEA");
}

#[test]
fn a_lead_registers_only_into_departments_they_lead_when_the_member_is_stored() {
    let (db, _, _) = bunker();
    let sales = add(&db, "Sales");
    let pam = hex();
    db.add_team_member("Pam", &pam, Role::Signer).unwrap();
    db.grant_identity_as(Some(&sales), &pam, DepartmentRole::Lead, None, Utc::now()).unwrap();
    let named = [Some(sales.clone())];

    let paul = hex();
    let added = db.add_team_member_with("Paul", &paul, Role::Signer, Some(&pam), Some(&pam), Departments::Named(&named)).unwrap();
    assert!(matches!(added, AddMemberOutcome::Added(_)));
    assert!(db.may_use_identity(&paul, Some(&sales)).unwrap());
    // Named none, or the primary, which she does not lead.
    for departments in [Departments::Default, Departments::Named(&[]), Departments::Named(&[None])] {
        let key = hex();
        assert_eq!(db.add_team_member_with("X", &key, Role::Signer, Some(&pam), Some(&pam), departments).unwrap(), AddMemberOutcome::NotYourDepartment);
        assert!(db.find_member_by_pubkey(&key).unwrap().is_none());
    }

    // Once she is made a member, read in the transaction that would store Quinn.
    db.grant_identity_as(Some(&sales), &pam, DepartmentRole::Member, None, Utc::now()).unwrap();
    let quinn = hex();
    assert_eq!(db.add_team_member_with("Quinn", &quinn, Role::Signer, Some(&pam), Some(&pam), Departments::Named(&named)).unwrap(), AddMemberOutcome::NotYourDepartment);
    assert!(db.find_member_by_pubkey(&quinn).unwrap().is_none());
}

#[test]
fn whether_a_new_member_gets_the_primary_is_decided_when_they_are_stored() {
    let (db, admin, _) = bunker();
    // Before grants matter: no grant, and the primary like everyone.
    let early = hex();
    db.add_team_member_with("Early", &early, Role::Signer, Some(&admin), None, Departments::Default).unwrap();
    assert!(db.identity_grants(None).unwrap().is_empty());
    assert!(db.may_use_identity(&early, None).unwrap());

    // After the first lead, a registration naming none is granted the primary.
    let pam = hex();
    db.add_team_member("Pam", &pam, Role::Signer).unwrap();
    db.grant_identity_as(None, &pam, DepartmentRole::Lead, None, Utc::now()).unwrap();
    let late = hex();
    db.add_team_member_with("Late", &late, Role::Signer, Some(&admin), None, Departments::Default).unwrap();
    assert!(db.may_use_identity(&late, None).unwrap(), "granted the primary in the same transaction");
    assert!(db.may_use_identity(&early, None).unwrap(), "granted it when grants started to matter");
    // An administrator may register someone into no department.
    let none = hex();
    db.add_team_member_with("None", &none, Role::Signer, Some(&admin), None, Departments::Named(&[])).unwrap();
    assert!(!db.may_use_identity(&none, None).unwrap());
}

#[test]
fn a_former_lead_renames_nothing() {
    let (db, _, signer) = bunker();
    let sales = add(&db, "Sales");
    db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Lead, None, Utc::now()).unwrap();
    assert_eq!(db.update_identity(&sales, Some(&sales), Some("Mine"), None, Some(&signer), Some(&signer), Utc::now()).unwrap(), UpdateIdentityOutcome::Updated);
    db.grant_identity_as(Some(&sales), &signer, DepartmentRole::Member, None, Utc::now()).unwrap();
    assert_eq!(db.update_identity(&sales, Some(&sales), Some("Still mine"), None, Some(&signer), Some(&signer), Utc::now()).unwrap(), UpdateIdentityOutcome::NotALead);
    assert_eq!(db.find_identity(&sales).unwrap().unwrap().label, "Mine");
}

#[test]
fn a_department_and_its_access_mode_are_read_together() {
    let (db, _, signer) = bunker();
    assert_eq!(db.department_of(None).unwrap(), (vec![], false));
    db.grant_identity_as(None, &signer, DepartmentRole::Lead, None, Utc::now()).unwrap();
    let (grants, matter) = db.department_of(None).unwrap();
    assert!(matter);
    assert_eq!(grants.len(), 2, "the backfilled members with it: {:?}", grants);
}
