use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use uuid::{Uuid, Variant, Version};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("identifier must be a canonical lowercase UUIDv7")]
    InvalidUuid,
    #[error(
        "reference must be nonempty and contain no surrounding whitespace or controls"
    )]
    InvalidReference,
}

fn validate_uuid(value: &str) -> Result<(), IdentityError> {
    let parsed =
        Uuid::parse_str(value).map_err(|_| IdentityError::InvalidUuid)?;
    if value.len() != 36
        || parsed.hyphenated().to_string() != value
        || parsed.get_version() != Some(Version::SortRand)
        || parsed.get_variant() != Variant::RFC4122
    {
        return Err(IdentityError::InvalidUuid);
    }
    Ok(())
}

pub fn validate_reference(value: &str) -> Result<(), IdentityError> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(IdentityError::InvalidReference);
    }
    Ok(())
}

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn generate() -> Self {
                Self(Uuid::now_v7().to_string())
            }

            pub fn parse(
                value: impl Into<String>,
            ) -> Result<Self, IdentityError> {
                let value = value.into();
                validate_uuid(&value)?;
                Ok(Self(value))
            }

            pub const fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                Self::parse(String::deserialize(deserializer)?)
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

uuid_id!(AutomationId);
uuid_id!(RunId);
uuid_id!(AttemptId);
uuid_id!(GrantId);
uuid_id!(DeletionPlanId);
// A generation epoch is an operation nonce, not a product entity identity.
uuid_id!(GenerationEpoch);

macro_rules! opaque_ref {
    ($name:ident) => {
        #[derive(
            Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(
                value: impl Into<String>,
            ) -> Result<Self, IdentityError> {
                let value = value.into();
                validate_reference(&value)?;
                Ok(Self(value))
            }

            pub const fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                Self::parse(String::deserialize(deserializer)?)
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

// Existing host protocol references are not re-encoded during this addition.
opaque_ref!(SessionRef);
opaque_ref!(SubmissionRef);
opaque_ref!(RequestKey);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PrincipalId {
    authority: String,
    external_user_id: i64,
}

#[derive(Deserialize)]
struct PrincipalWire {
    authority: String,
    external_user_id: i64,
}

impl PrincipalId {
    /// `authority` is the host-selected account namespace; the user ID is
    /// issued by that authority and is not an automation business UUID.
    pub fn cloud(
        authority: impl Into<String>,
        external_user_id: i64,
    ) -> Result<Self, IdentityError> {
        let authority = authority.into();
        validate_reference(&authority)?;
        Ok(Self {
            authority,
            external_user_id,
        })
    }

    pub const fn authority(&self) -> &str {
        self.authority.as_str()
    }

    pub const fn external_user_id(&self) -> i64 {
        self.external_user_id
    }
}

impl fmt::Debug for PrincipalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrincipalId(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for PrincipalId {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let wire = PrincipalWire::deserialize(deserializer)?;
        Self::cloud(wire.authority, wire.external_user_id)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountGeneration {
    pub epoch: GenerationEpoch,
    pub sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_ids_reject_legacy_and_noncanonical_forms_on_parse_and_serde() {
        let valid = "0190f5fe-7c00-7a00-8000-000000000003";
        assert_eq!(RunId::parse(valid).unwrap().as_str(), valid);
        assert_eq!(
            serde_json::from_str::<RunId>(&format!("\"{valid}\""))
                .unwrap()
                .as_str(),
            valid
        );
        for value in [
            "0190F5FE-7C00-7A00-8000-000000000003",
            "0190f5fe-7c00-4a00-8000-000000000003",
            "0190f5fe-7c00-7a00-c000-000000000003",
            "run_0190f5fe-7c00-7a00-8000-000000000003",
            "0190f5fe7c007a008000000000000003",
            " 0190f5fe-7c00-7a00-8000-000000000003",
            "short",
        ] {
            assert!(RunId::parse(value).is_err());
            assert!(
                serde_json::from_str::<RunId>(
                    &serde_json::to_string(value).unwrap()
                )
                .is_err()
            );
        }
        assert!(RunId::parse(RunId::generate().as_str()).is_ok());
    }

    #[test]
    fn principal_namespace_is_part_of_identity_and_debug_is_private() {
        let first =
            PrincipalId::cloud("https://account.example/tenant-a", 42).unwrap();
        let second =
            PrincipalId::cloud("https://account.example/tenant-b", 42).unwrap();
        assert_ne!(first, second);
        assert!(!format!("{first:?}").contains("account.example"));
        assert!(PrincipalId::cloud("", 42).is_err());
        assert!(
            serde_json::from_str::<PrincipalId>(
                r#"{"authority":" ","external_user_id":42}"#
            )
            .is_err()
        );
        assert_eq!(
            SessionRef::parse("session-legacy").unwrap().as_str(),
            "session-legacy"
        );
        assert_eq!(
            SubmissionRef::parse("submission-0012").unwrap().as_str(),
            "submission-0012"
        );
    }
}
