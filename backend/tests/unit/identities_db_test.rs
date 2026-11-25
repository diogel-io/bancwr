// Identities and grants in the database (#103): the primary recorded at start, adding another,
// who may use which, and a single-key bunker unchanged.
use bunker::db::{AddIdentityRefusal, Database};
use bunker::registry::Role;
use chrono::Utc;
use nostr::prelude::*;

fn db() -> Database {
    Database::new(":memory:").unwrap()
}

fn hex() -> String {
    Keys::generate().public_key().to_hex()
}

#[test]
fn the_primary_is_recorded_once_and_follows_the_key_file() {
    let db = db();
    let (first, second) = (hex(), hex());
    db.ensure_primary_identity(&first, Utc::now()).unwrap();
    db.ensure_primary_identity(&first, Utc::now()).unwrap();
    let listed = db.list_identities().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].pubkey.as_str(), listed[0].label.as_str(), listed[0].is_primary), (first.as_str(), "Primary", true));
    assert_eq!(listed[0].reference(), None);
    // A restore of another bunker's key file: the primary row follows it.
    db.ensure_primary_identity(&second, Utc::now()).unwrap();
    assert_eq!(db.list_identities().unwrap()[0].pubkey, second);
}

#[test]
fn with_only_the_primary_everyone_may_use_it_as_before() {
    let db = db();
    db.ensure_primary_identity(&hex(), Utc::now()).unwrap();
    let signer = hex();
    db.add_team_member("Sam", &signer, Role::Signer).unwrap();
    assert!(!db.has_several_identities().unwrap());
    assert!(db.may_use_identity(&signer, None).unwrap());
}

#[test]
fn the_first_identity_added_grants_the_primary_to_everyone_who_may_sign() {
    let db = db();
    db.ensure_primary_identity(&hex(), Utc::now()).unwrap();
    let (admin, signer) = (hex(), hex());
    db.add_team_member("Ann", &admin, Role::Administrator).unwrap();
    db.add_team_member("Sam", &signer, Role::Signer).unwrap();
    let brand = hex();
    assert_eq!(db.add_identity(&brand, "Brand B", "line", Utc::now()).unwrap(), Ok(()));
    assert!(db.has_several_identities().unwrap());

    let mut granted = db.identity_members(None).unwrap();
    granted.sort();
    let mut expected = vec![admin.clone(), signer.clone()];
    expected.sort();
    assert_eq!(granted, expected, "existing members who sign keep the primary");
    assert!(db.may_use_identity(&signer, None).unwrap());
    assert!(!db.may_use_identity(&signer, Some(&brand)).unwrap());
    // Administrators may use every identity without a grant.
    assert!(db.may_use_identity(&admin, Some(&brand)).unwrap());

    // A member added later has nothing until granted.
    let late = hex();
    db.add_team_member("Lee", &late, Role::Signer).unwrap();
    assert!(!db.may_use_identity(&late, None).unwrap());
    assert!(db.grant_identity(Some(&brand), &late, Some(&admin), Utc::now()).unwrap());
    assert!(!db.grant_identity(Some(&brand), &late, Some(&admin), Utc::now()).unwrap(), "granted once");
    assert!(db.may_use_identity(&late, Some(&brand)).unwrap());

    let stored = db.stored_identities().unwrap();
    assert_eq!((stored.len(), stored[0].pubkey.as_str(), stored[0].key_file.as_str()), (1, brand.as_str(), "line"));
    let added = db.audit_events().unwrap().into_iter().find(|e| e.action == "identity_added").unwrap();
    assert_eq!((added.identity_pubkey.as_deref(), added.detail.as_deref(), added.actor_pubkey), (Some(brand.as_str()), Some("label=Brand B"), None));
}

#[test]
fn an_identity_already_held_or_a_members_key_is_refused() {
    let db = db();
    let primary = hex();
    db.ensure_primary_identity(&primary, Utc::now()).unwrap();
    let member = hex();
    db.add_team_member("Sam", &member, Role::Signer).unwrap();
    assert_eq!(db.add_identity(&primary, "x", "line", Utc::now()).unwrap(), Err(AddIdentityRefusal::AlreadyAnIdentity));
    assert_eq!(db.add_identity(&member, "x", "line", Utc::now()).unwrap(), Err(AddIdentityRefusal::IsAMember));
    let brand = hex();
    db.add_identity(&brand, "Brand B", "line", Utc::now()).unwrap().unwrap();
    assert_eq!(db.add_identity(&brand, "Again", "line", Utc::now()).unwrap(), Err(AddIdentityRefusal::AlreadyAnIdentity));
}

#[test]
fn removing_a_member_removes_their_grants() {
    let db = db();
    db.ensure_primary_identity(&hex(), Utc::now()).unwrap();
    db.add_team_member("Ann", &hex(), Role::Administrator).unwrap();
    let brand = hex();
    db.add_identity(&brand, "Brand B", "line", Utc::now()).unwrap().unwrap();
    let signer = hex();
    let id = db.add_team_member("Sam", &signer, Role::Signer).unwrap();
    db.grant_identity(Some(&brand), &signer, None, Utc::now()).unwrap();
    db.remove_team_member(id).unwrap();
    assert!(db.identity_members(Some(&brand)).unwrap().is_empty());
}
