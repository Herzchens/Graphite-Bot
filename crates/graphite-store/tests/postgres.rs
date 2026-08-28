use graphite_store::{PgStore, TosDocument};
use uuid::Uuid;

#[tokio::test]
async fn registration_is_idempotent_and_starter_safe() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();
    store
        .ensure_tos_document(&TosDocument {
            version: 1,
            document_url: "https://example.invalid/tos/v1".to_owned(),
            document_sha256: [9; 32],
        })
        .await
        .unwrap();

    let uuid = Uuid::now_v7();
    let raw = u64::from_be_bytes(uuid.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = (raw % 8_000_000_000_000_000_000_u64).max(1);
    let request_key = format!("test:register:{uuid}");
    let fingerprint = [7_u8; 32];

    let first = store
        .register_player(discord_user_id, 1, &fingerprint, &request_key)
        .await
        .unwrap();
    let retry = store
        .register_player(discord_user_id, 1, &fingerprint, &request_key)
        .await
        .unwrap();

    assert_eq!(first, retry);
    assert_eq!(first.starter_item_count, 7);

    store
        .ensure_tos_document(&TosDocument {
            version: 2,
            document_url: "https://example.invalid/tos/v2".to_owned(),
            document_sha256: [10; 32],
        })
        .await
        .unwrap();

    let retry_after_policy_rotation = store
        .register_player(discord_user_id, 1, &fingerprint, &request_key)
        .await
        .unwrap();
    assert_eq!(retry_after_policy_rotation, first);

    let profile = store
        .profile_for_discord(discord_user_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(profile.player_id, first.player_id);
    assert_eq!(profile.starter_item_count, 7);
    assert_eq!(profile.wallet.get(), 0);
    assert_eq!(profile.bank.get(), 0);
    assert_eq!(profile.liability.get(), 0);

    let second_key = format!("test:register-again:{uuid}");
    let repeated_registration = store
        .register_player(discord_user_id, 2, &fingerprint, &second_key)
        .await
        .unwrap();
    assert_eq!(repeated_registration.player_id, first.player_id);
    assert_eq!(repeated_registration.starter_item_count, 7);
}
