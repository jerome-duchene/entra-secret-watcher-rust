use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialType {
    Secret,
    Certificate,
}

impl CredentialType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Certificate => "certificate",
        }
    }
}

impl std::fmt::Display for CredentialType {
    /// Same casing as the C# enum (`Secret`, `Certificate`), used in messages.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Secret => f.write_str("Secret"),
            Self::Certificate => f.write_str("Certificate"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialStatus {
    Expired,
    ExpiringSoon,
    Valid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpiringCredential {
    pub app_name: String,
    pub app_id: String,
    /// Entra key identifier (GUID) of the secret or certificate. Unique per credential,
    /// unlike the display name which can be empty or duplicated.
    pub key_id: String,
    pub credential_type: CredentialType,
    pub credential_name: String,
    pub expires_on: DateTime<Utc>,
    pub days_left: i64,
}

impl ExpiringCredential {
    pub fn status(&self) -> CredentialStatus {
        match self.days_left {
            d if d < 0 => CredentialStatus::Expired,
            d if d <= 30 => CredentialStatus::ExpiringSoon,
            _ => CredentialStatus::Valid,
        }
    }

    pub fn status_label(&self) -> String {
        match self.status() {
            CredentialStatus::Expired => format!("EXPIRED since {} day(s)", self.days_left.abs()),
            CredentialStatus::ExpiringSoon => format!("Expires in {} day(s)", self.days_left),
            CredentialStatus::Valid => "Valid".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScanResult {
    pub tenant_name: String,
    pub tenant_id: String,
    pub scanned_at: DateTime<Utc>,
    pub credentials: Vec<ExpiringCredential>,
    pub total_applications_scanned: usize,
}

impl ScanResult {
    pub fn has_expiring(&self) -> bool {
        !self.credentials.is_empty()
    }

    pub fn expired_count(&self) -> usize {
        self.count(CredentialStatus::Expired)
    }

    pub fn expiring_soon_count(&self) -> usize {
        self.count(CredentialStatus::ExpiringSoon)
    }

    fn count(&self, status: CredentialStatus) -> usize {
        self.credentials.iter().filter(|c| c.status() == status).count()
    }

    /// The same scan restricted to the given credentials (used for ungrouped reports).
    pub fn with_credentials(&self, credentials: Vec<ExpiringCredential>) -> Self {
        Self { credentials, ..self.clone() }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use chrono::TimeZone;

    pub fn credential(days_left: i64) -> ExpiringCredential {
        ExpiringCredential {
            app_name: "My App".into(),
            app_id: "app-id".into(),
            key_id: "key-id".into(),
            credential_type: CredentialType::Secret,
            credential_name: "prod-secret".into(),
            expires_on: Utc.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap(),
            days_left,
        }
    }

    pub fn scan(credentials: Vec<ExpiringCredential>) -> ScanResult {
        ScanResult {
            tenant_name: "Contoso".into(),
            tenant_id: "tenant-id".into(),
            scanned_at: Utc.with_ymd_and_hms(2026, 10, 17, 3, 0, 0).unwrap(),
            credentials,
            total_applications_scanned: 12,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn status_follows_days_left() {
        assert_eq!(credential(-1).status(), CredentialStatus::Expired);
        assert_eq!(credential(0).status(), CredentialStatus::ExpiringSoon);
        assert_eq!(credential(30).status(), CredentialStatus::ExpiringSoon);
        assert_eq!(credential(31).status(), CredentialStatus::Valid);
    }

    #[test]
    fn status_label_describes_the_delay() {
        assert_eq!(credential(-3).status_label(), "EXPIRED since 3 day(s)");
        assert_eq!(credential(8).status_label(), "Expires in 8 day(s)");
        assert_eq!(credential(45).status_label(), "Valid");
    }

    #[test]
    fn scan_result_counts_by_status() {
        let result = scan(vec![credential(-5), credential(-1), credential(7)]);
        assert!(result.has_expiring());
        assert_eq!(result.expired_count(), 2);
        assert_eq!(result.expiring_soon_count(), 1);
        assert!(!scan(vec![]).has_expiring());
    }
}
