use anyhow::Context;
use zeroize::Zeroize;

const SERVICE: &str = "openape";
const ACCOUNT: &str = "moodle-auto-login";

pub struct StoredCredentials {
    pub id: String,
    pub password: String,
}

impl StoredCredentials {
    pub fn new(mut id: String, mut password: String) -> anyhow::Result<Self> {
        if id.trim().is_empty() || password.is_empty() {
            id.zeroize();
            password.zeroize();
            anyhow::bail!("Student ID and password must not be empty.");
        }

        let normalized_id = id.trim().to_owned();
        id.zeroize();
        Ok(Self {
            id: normalized_id,
            password,
        })
    }

    fn entry() -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, ACCOUNT).context("Failed to open OS credential store")
    }

    fn encode(&self) -> anyhow::Result<String> {
        serde_json::to_string(&(&self.id, &self.password)).context("Failed to encode credentials")
    }

    fn decode(payload: &str) -> anyhow::Result<Self> {
        let (id, password): (String, String) =
            serde_json::from_str(payload).context("Invalid credential store payload")?;
        Self::new(id, password)
    }

    pub fn load() -> anyhow::Result<Option<Self>> {
        match Self::entry()?.get_password() {
            Ok(mut payload) => {
                let result = Self::decode(&payload).map(Some);
                payload.zeroize();
                result
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error).context("Failed to read OS credential store"),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let mut payload = self.encode()?;
        let result = match Self::entry() {
            Ok(entry) => entry
                .set_password(&payload)
                .context("Failed to save credentials to OS credential store"),
            Err(error) => Err(error),
        };
        payload.zeroize();
        result
    }

    pub fn delete() -> anyhow::Result<()> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error).context("Failed to delete OS credential"),
        }
    }
}

impl Drop for StoredCredentials {
    fn drop(&mut self) {
        self.id.zeroize();
        self.password.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::StoredCredentials;

    #[test]
    fn credential_payload_round_trips_special_characters() {
        let credentials =
            StoredCredentials::new("11234567".into(), "p@ss\"\\\nword".into()).unwrap();
        let decoded = StoredCredentials::decode(&credentials.encode().unwrap()).unwrap();

        assert_eq!(decoded.id, credentials.id);
        assert_eq!(decoded.password, credentials.password);
    }
}
