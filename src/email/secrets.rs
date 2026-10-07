//! Dedicated gateway credential boundary.
//!
//! EML-100 owns no credentials. EML-101 resolves one profile/account-scoped
//! app password from a distinct macOS Keychain service through this injectable
//! interface; raw bytes never enter configuration or formatting surfaces.

use std::sync::Arc;

#[allow(dead_code)] // Retained EML-100 namespace compatibility.
pub const EMAIL_SECRET_SERVICE: &str = "ca.email";
#[cfg(any(target_os = "macos", test))]
pub const GATEWAY_SECRET_SERVICE: &str = "ca.email.gateway";
const GATEWAY_REFERENCE_PREFIX: &str = "keychain://ca/email-gateway/";

#[allow(dead_code)] // Retained EML-100 namespace compatibility.
pub fn account_name(profile: &str, account_id: &str) -> String {
    format!("{profile}:email:{account_id}")
}

pub fn gateway_reference(account_id: &str) -> String {
    format!("{GATEWAY_REFERENCE_PREFIX}{account_id}")
}

pub fn gateway_reference_account(value: &str) -> Option<&str> {
    let account = value.strip_prefix(GATEWAY_REFERENCE_PREFIX)?;
    valid_component(account).then_some(account)
}

pub fn gateway_keychain_account(profile: &str, account_id: &str) -> String {
    format!("{profile}:gateway:{account_id}")
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

pub trait GatewaySecretStore: Send + Sync {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>>;
    #[allow(dead_code)] // Reserved for an explicit credential-management UI.
    fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()>;
    #[allow(dead_code)] // Reserved for an explicit credential-management UI.
    fn delete(&self, account: &str) -> anyhow::Result<()>;
}

pub fn system_gateway_secret_store() -> Arc<dyn GatewaySecretStore> {
    Arc::new(SystemGatewaySecretStore)
}

struct SystemGatewaySecretStore;

#[cfg(target_os = "macos")]
impl GatewaySecretStore for SystemGatewaySecretStore {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let options = security_framework::passwords::PasswordOptions::new_generic_password(
            GATEWAY_SECRET_SERVICE,
            account,
        );
        match security_framework::passwords::generic_password(options) {
            Ok(secret) => Ok(Some(secret)),
            Err(error) if error.code() == -25_300 => Ok(None),
            Err(_) => anyhow::bail!("macOS Keychain could not read email gateway authorization"),
        }
    }

    fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
        security_framework::passwords::set_generic_password(GATEWAY_SECRET_SERVICE, account, secret)
            .map_err(|_| {
                anyhow::anyhow!("macOS Keychain could not save email gateway authorization")
            })
    }

    fn delete(&self, account: &str) -> anyhow::Result<()> {
        match security_framework::passwords::delete_generic_password(
            GATEWAY_SECRET_SERVICE,
            account,
        ) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25_300 => Ok(()),
            Err(_) => anyhow::bail!("macOS Keychain could not delete email gateway authorization"),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl GatewaySecretStore for SystemGatewaySecretStore {
    fn load(&self, _account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        anyhow::bail!("email gateway authorization requires macOS Keychain")
    }

    fn store(&self, _account: &str, _secret: &[u8]) -> anyhow::Result<()> {
        anyhow::bail!("email gateway authorization requires macOS Keychain")
    }

    fn delete(&self, _account: &str) -> anyhow::Result<()> {
        anyhow::bail!("email gateway authorization requires macOS Keychain")
    }
}

pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> anyhow::Result<Self> {
        if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
            anyhow::bail!("email gateway authorization is invalid");
        }
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretBytes([REDACTED])")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_and_references_are_distinct_and_redacted() {
        assert_eq!(EMAIL_SECRET_SERVICE, "ca.email");
        assert_eq!(GATEWAY_SECRET_SERVICE, "ca.email.gateway");
        assert_eq!(account_name("work", "gateway"), "work:email:gateway");
        assert_eq!(
            gateway_reference_account(&gateway_reference("gateway")),
            Some("gateway")
        );
        assert_eq!(
            gateway_reference_account("keychain://ca/provider/gateway"),
            None
        );
        let secret = SecretBytes::new(b"sentinel-password".to_vec()).unwrap();
        assert_eq!(format!("{secret:?}"), "SecretBytes([REDACTED])");
    }
}
