use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityError(String);

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for IdentityError {}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

macro_rules! validated_string {
    ($name:ident, $validator:expr, $message:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: String) -> Result<Self, IdentityError> {
                if !($validator)(&value) {
                    return Err(IdentityError($message.to_string()));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

validated_string!(Sha256Digest, valid_digest, "invalid SHA-256 digest");
validated_string!(
    UuidDigest,
    |value: &str| value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
    "invalid UUID digest"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_reject_uppercase_and_wrong_length() {
        assert!(Sha256Digest::parse("a".repeat(64)).is_ok());
        assert!(Sha256Digest::parse("A".repeat(64)).is_err());
        assert!(Sha256Digest::parse("a".repeat(63)).is_err());
        assert!(UuidDigest::parse("b".repeat(32)).is_ok());
        assert!(UuidDigest::parse("b".repeat(31)).is_err());
        assert!(UuidDigest::parse("g".repeat(32)).is_err());
    }
}
