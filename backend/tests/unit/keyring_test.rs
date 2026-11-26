// The keyring (#103): the primary and the identities beside it, looked up by reference and by the
// key a request is addressed to.
use bunker::keyring::Keyring;
use bunker::signer::Signer;
use nostr::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;

fn keyring(primary: &Keys, others: &[&Keys]) -> Keyring {
    let primary = Arc::new(RwLock::new(Signer::new(primary.secret_key().clone())));
    Keyring::new(primary, others.iter().map(|k| Signer::new(k.secret_key().clone())).collect())
}

#[tokio::test]
async fn a_single_key_bunker_has_only_the_primary() {
    let primary = Keys::generate();
    let ring = keyring(&primary, &[]);
    assert_eq!(ring.others(), 0);
    assert_eq!(ring.signer(None).await.unwrap().public_key(), primary.public_key());
    assert_eq!(ring.public_keys().await, vec![primary.public_key()]);
    assert_eq!(ring.identity_of(&primary.public_key()).await, Some(None));
    assert_eq!(ring.identity_of(&Keys::generate().public_key()).await, None);
}

#[tokio::test]
async fn each_identity_signs_as_itself_and_an_unknown_one_is_none() {
    let (primary, brand) = (Keys::generate(), Keys::generate());
    let ring = keyring(&primary, &[&brand]);
    let hex = brand.public_key().to_hex();
    assert_eq!(ring.signer(Some(&hex)).await.unwrap().public_key(), brand.public_key());
    assert!(ring.signer(Some(&Keys::generate().public_key().to_hex())).await.is_none());
    assert_eq!(ring.identity_of(&brand.public_key()).await, Some(Some(hex.clone())));
    assert_eq!(ring.public_keys().await, vec![primary.public_key(), brand.public_key()]);
    assert!(ring.is_identity(&hex).await && ring.is_identity(&primary.public_key().to_hex()).await);

    let event = ring.signer(Some(&hex)).await.unwrap().sign_event(EventBuilder::text_note("x").build(brand.public_key())).await.unwrap();
    assert_eq!(event.pubkey, brand.public_key());
    assert!(event.verify().is_ok());
}

#[tokio::test]
async fn an_identity_can_be_added_and_removed_while_the_bunker_runs() {
    let primary = Keys::generate();
    let sales = Keys::generate();
    let ring = keyring(&primary, &[]);
    let hex = sales.public_key().to_hex();

    assert!(ring.add(Signer::new(sales.secret_key().clone())).await);
    assert_eq!(ring.others(), 1);
    assert!(ring.is_identity(&hex).await);
    assert_eq!(ring.identity_of(&sales.public_key()).await, Some(Some(hex.clone())));
    assert_eq!(ring.public_keys().await, vec![primary.public_key(), sales.public_key()]);
    // Once only, and never the primary.
    assert!(!ring.add(Signer::new(sales.secret_key().clone())).await);
    assert!(!ring.add(Signer::new(primary.secret_key().clone())).await);
    assert_eq!(ring.others(), 1);

    assert!(ring.remove(&hex));
    assert!(!ring.remove(&hex));
    assert!(!ring.is_identity(&hex).await);
    assert!(ring.signer(Some(&hex)).await.is_none());
    assert_eq!(ring.identity_of(&sales.public_key()).await, None);
    // The primary is never in the map, so removing it changes nothing.
    assert!(!ring.remove(&primary.public_key().to_hex()));
    assert_eq!(ring.public_keys().await, vec![primary.public_key()]);
}
