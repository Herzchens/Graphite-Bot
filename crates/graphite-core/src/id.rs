use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PlayerId(Uuid);

impl PlayerId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for PlayerId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OperationId(Uuid);

impl OperationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct IdentityFingerprint([u8; 32]);

impl IdentityFingerprint {
    #[must_use]
    pub fn for_discord_user(key: &[u8], discord_user_id: u64) -> Self {
        type HmacSha256 = Hmac<Sha256>;

        let mut mac =
            HmacSha256::new_from_slice(key).expect("HMAC accepts keys of arbitrary length");
        mac.update(b"graphite.deletion-cooldown.v1\0");
        mac.update(&discord_user_id.to_be_bytes());

        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(&mac.finalize().into_bytes());
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_uuid_v7() {
        assert_eq!(PlayerId::new().as_uuid().get_version_num(), 7);
        assert_eq!(OperationId::new().as_uuid().get_version_num(), 7);
    }

    #[test]
    fn identity_fingerprint_is_keyed_and_stable() {
        let a = IdentityFingerprint::for_discord_user(b"key-a", 123);
        let b = IdentityFingerprint::for_discord_user(b"key-a", 123);
        let c = IdentityFingerprint::for_discord_user(b"key-b", 123);

        assert_eq!(a.as_bytes(), b.as_bytes());
        assert_ne!(a.as_bytes(), c.as_bytes());
    }
}
