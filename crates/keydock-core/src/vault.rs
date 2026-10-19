use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

pub const VAULT_VERSION: u32 = 1;
pub const KDF_LOG_N: u8 = 14;
pub const KDF_R: u32 = 8;
pub const KDF_P: u32 = 1;
pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("凭据库尚未解锁")]
    Locked,
    #[error("主密码错误")]
    WrongPassphrase,
    #[error("凭据库文件不存在")]
    Missing,
    #[error("凭据库格式无效: {0}")]
    Invalid(String),
    #[error("凭据库 i/o 错误: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfSection {
    pub algo: String,
    pub salt: String,
    pub log_n: u8,
    pub r: u32,
    pub p: u32,
    pub keylen: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CipherSection {
    pub algo: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEnvelope {
    pub version: u32,
    pub kdf: KdfSection,
    pub cipher: CipherSection,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultEntry {
    pub password: String,
}

impl Drop for VaultEntry {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultContents {
    pub version: u32,
    pub updated_at: String,
    pub entries: BTreeMap<String, VaultEntry>,
}

/// An opened vault file. The decrypted contents live only in memory and are
/// zeroized when the vault is locked.
pub struct Vault {
    path: PathBuf,
    kdf: KdfSection,
    key: Option<Zeroizing<Vec<u8>>>,
    contents: Option<VaultContents>,
}

impl Vault {
    pub fn exists(path: &Path) -> bool {
        path.is_file()
    }

    /// Read the envelope. Returns `None` when the vault file does not exist.
    pub fn open(path: &Path) -> Result<Option<Self>, VaultError> {
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(path)?;
        let envelope: VaultEnvelope = serde_json::from_slice(&bytes)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        if envelope.version != VAULT_VERSION {
            return Err(VaultError::Invalid(format!(
                "unsupported vault version {}",
                envelope.version
            )));
        }
        if envelope.kdf.algo != "scrypt" || envelope.cipher.algo != "aes-256-gcm" {
            return Err(VaultError::Invalid("unsupported algorithm".into()));
        }
        let salt = B64
            .decode(&envelope.kdf.salt)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        if salt.len() != SALT_LEN {
            return Err(VaultError::Invalid("bad salt length".into()));
        }
        Ok(Some(Self {
            path: path.to_path_buf(),
            kdf: envelope.kdf,
            key: None,
            contents: None,
        }))
    }

    /// Create a brand-new vault, leaving it unlocked.
    pub fn create(path: &Path, passphrase: &str) -> Result<Self, VaultError> {
        validate_passphrase(passphrase)?;
        let salt = random_bytes(SALT_LEN)?;
        let key = derive_key(passphrase, &salt, KDF_LOG_N, KDF_R, KDF_P, KEY_LEN)?;
        let contents = VaultContents {
            version: VAULT_VERSION,
            updated_at: now(),
            entries: BTreeMap::new(),
        };
        let mut vault = Self {
            path: path.to_path_buf(),
            kdf: KdfSection {
                algo: "scrypt".into(),
                salt: B64.encode(salt),
                log_n: KDF_LOG_N,
                r: KDF_R,
                p: KDF_P,
                keylen: KEY_LEN as u32,
            },
            key: Some(Zeroizing::new(key)),
            contents: Some(contents),
        };
        vault.save()?;
        Ok(vault)
    }

    pub fn is_unlocked(&self) -> bool {
        self.key.is_some() && self.contents.is_some()
    }

    pub fn unlock(&mut self, passphrase: &str) -> Result<(), VaultError> {
        let salt = B64
            .decode(&self.kdf.salt)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        let key = derive_key(
            passphrase,
            &salt,
            self.kdf.log_n,
            self.kdf.r,
            self.kdf.p,
            self.kdf.keylen as usize,
        )?;
        self.unlock_with_key(&key)
    }

    /// Unlock with a previously derived 32-byte key (system keychain path).
    pub fn unlock_with_key(&mut self, key: &[u8]) -> Result<(), VaultError> {
        if key.len() != self.kdf.keylen as usize {
            return Err(VaultError::Invalid("key length mismatch".into()));
        }
        let contents = self.decrypt(key)?;
        self.key = Some(Zeroizing::new(key.to_vec()));
        self.contents = Some(contents);
        Ok(())
    }

    /// Base64 of the derived key, for storing in the OS keychain only after the
    /// user explicitly enables quick unlock.
    pub fn derived_key_b64(&self) -> Option<String> {
        self.key.as_ref().map(|key| B64.encode(key.as_slice()))
    }

    pub fn lock(&mut self) {
        self.contents = None;
        self.key = None;
    }

    pub fn has(&self, emp_no: &str) -> bool {
        self.contents
            .as_ref()
            .map(|contents| contents.entries.contains_key(&credential_key(emp_no)))
            .unwrap_or(false)
    }

    pub fn credential(&self, emp_no: &str) -> Option<&str> {
        self.contents
            .as_ref()
            .and_then(|contents| contents.entries.get(&credential_key(emp_no)))
            .map(|entry| entry.password.as_str())
    }

    pub fn shared_password(&self) -> Option<&str> {
        self.contents
            .as_ref()?
            .entries
            .get("shared:password")
            .map(|entry| entry.password.as_str())
    }

    pub fn set_credential(&mut self, emp_no: &str, password: &str) -> Result<(), VaultError> {
        if !self.is_unlocked() {
            return Err(VaultError::Locked);
        }
        let key = credential_key(emp_no);
        let contents = self.contents.as_mut().ok_or(VaultError::Locked)?;
        contents.entries.insert(
            key,
            VaultEntry {
                password: password.to_owned(),
            },
        );
        contents.updated_at = now();
        self.save()
    }

    pub fn clear_credential(&mut self, emp_no: &str) -> Result<(), VaultError> {
        if !self.is_unlocked() {
            return Err(VaultError::Locked);
        }
        let key = credential_key(emp_no);
        let contents = self.contents.as_mut().ok_or(VaultError::Locked)?;
        contents.entries.remove(&key);
        contents.updated_at = now();
        self.save()
    }

    pub fn rename_credential(
        &mut self,
        old_emp_no: &str,
        new_emp_no: &str,
    ) -> Result<(), VaultError> {
        if !self.is_unlocked() {
            return Err(VaultError::Locked);
        }
        let old_key = credential_key(old_emp_no);
        let new_key = credential_key(new_emp_no);
        let contents = self.contents.as_mut().ok_or(VaultError::Locked)?;
        if let Some(entry) = contents.entries.remove(&old_key) {
            contents.entries.insert(new_key, entry);
            contents.updated_at = now();
            self.save()?;
        }
        Ok(())
    }

    pub fn set_shared_password(&mut self, password: &str) -> Result<(), VaultError> {
        self.set_credential("shared:password", password)
    }

    pub fn change_passphrase(&mut self, current: &str, next: &str) -> Result<(), VaultError> {
        if !self.is_unlocked() {
            return Err(VaultError::Locked);
        }
        validate_passphrase(next)?;
        let salt = B64
            .decode(&self.kdf.salt)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        let current_key = derive_key(
            current,
            &salt,
            self.kdf.log_n,
            self.kdf.r,
            self.kdf.p,
            self.kdf.keylen as usize,
        )?;
        // Verify against disk rather than trusting in-memory state.
        let contents = self.decrypt(&current_key)?;
        let new_salt = random_bytes(SALT_LEN)?;
        let new_key = derive_key(next, &new_salt, KDF_LOG_N, KDF_R, KDF_P, KEY_LEN)?;
        self.kdf = KdfSection {
            algo: "scrypt".into(),
            salt: B64.encode(&new_salt),
            log_n: KDF_LOG_N,
            r: KDF_R,
            p: KDF_P,
            keylen: KEY_LEN as u32,
        };
        self.key = Some(Zeroizing::new(new_key));
        self.contents = Some(contents);
        self.save()
    }

    /// Delete the vault file. Station/account configuration is untouched.
    pub fn reset(path: &Path) -> Result<(), VaultError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(VaultError::Io(error)),
        }
    }

    fn save(&mut self) -> Result<(), VaultError> {
        let key = self.key.as_ref().ok_or(VaultError::Locked)?;
        let contents = self.contents.as_ref().ok_or(VaultError::Locked)?;
        let updated_at = now();
        let mut contents = contents.clone();
        contents.updated_at = updated_at.clone();
        let mut plaintext = serde_json::to_vec(&contents)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        let nonce = random_bytes(NONCE_LEN)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                aes_gcm::aead::Payload {
                    msg: &plaintext,
                    aad: b"keydock-vault-v1",
                },
            )
            .map_err(|_| VaultError::Invalid("encryption failed".into()))?;
        plaintext.zeroize();
        let envelope = VaultEnvelope {
            version: VAULT_VERSION,
            kdf: self.kdf.clone(),
            cipher: CipherSection {
                algo: "aes-256-gcm".into(),
                nonce: B64.encode(nonce),
                ciphertext: B64.encode(ciphertext),
            },
            updated_at,
        };
        let bytes = serde_json::to_vec_pretty(&envelope)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        crate::paths::atomic_write(&self.path, &bytes)?;
        if let Some(current) = self.contents.as_mut() {
            current.updated_at = contents.updated_at;
        }
        Ok(())
    }

    fn decrypt(&self, key: &[u8]) -> Result<VaultContents, VaultError> {
        let bytes = std::fs::read(&self.path)?;
        let envelope: VaultEnvelope = serde_json::from_slice(&bytes)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        let nonce = B64
            .decode(&envelope.cipher.nonce)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        let ciphertext = B64
            .decode(&envelope.cipher.ciphertext)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        if nonce.len() != NONCE_LEN {
            return Err(VaultError::Invalid("bad nonce length".into()));
        }
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                aes_gcm::aead::Payload {
                    msg: &ciphertext,
                    aad: b"keydock-vault-v1",
                },
            )
            .map_err(|_| VaultError::WrongPassphrase)?;
        let contents: VaultContents = serde_json::from_slice(&plaintext)
            .map_err(|error| VaultError::Invalid(error.to_string()))?;
        if contents.version != VAULT_VERSION {
            return Err(VaultError::Invalid("bad plaintext version".into()));
        }
        Ok(contents)
    }
}

pub fn validate_passphrase(passphrase: &str) -> Result<(), VaultError> {
    if passphrase.chars().count() < 8 {
        return Err(VaultError::Invalid("主密码至少 8 位".into()));
    }
    Ok(())
}

pub fn passphrase_strength_warning(passphrase: &str) -> bool {
    passphrase.chars().count() < 12
}

pub fn credential_key(emp_no: &str) -> String {
    if emp_no.starts_with("cred:") || emp_no.starts_with("shared:") {
        emp_no.to_owned()
    } else {
        format!("cred:{emp_no}")
    }
}

fn derive_key(
    passphrase: &str,
    salt: &[u8],
    log_n: u8,
    r: u32,
    p: u32,
    len: usize,
) -> Result<Vec<u8>, VaultError> {
    let params = scrypt::Params::new(log_n, r, p, len)
        .map_err(|error| VaultError::Invalid(error.to_string()))?;
    let mut output = vec![0_u8; len];
    scrypt::scrypt(passphrase.as_bytes(), salt, &params, &mut output)
        .map_err(|error| VaultError::Invalid(error.to_string()))?;
    Ok(output)
}

fn random_bytes(len: usize) -> Result<Vec<u8>, VaultError> {
    let mut buffer = vec![0_u8; len];
    getrandom::getrandom(&mut buffer)
        .map_err(|error| VaultError::Invalid(format!("random source failed: {error}")))?;
    Ok(buffer)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC7914_DKLEN: usize = 64;
    const RFC7914_EXPECTED: &str = "\
fd ba be 1c 9d 34 72 00 78 56 e7 19 0d 01 e9 fe \
7c 6a d7 cb c8 23 78 30 e7 73 76 63 4b 37 31 62 \
2e af 30 d9 2e 22 a3 88 6f f1 09 27 9d 98 30 da \
c7 27 af b9 4a 83 ee 6d 83 60 cb df a2 cc 06 40";

    fn temp_path(name: &str) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("keydock-vault-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        (root.join("vault.enc"), root)
    }

    #[test]
    fn scrypt_matches_rfc7914_vector() {
        let params = scrypt::Params::new(10, 8, 16, RFC7914_DKLEN).unwrap();
        let mut output = [0_u8; RFC7914_DKLEN];
        scrypt::scrypt(b"password", b"NaCl", &params, &mut output).unwrap();
        let hex = output
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(hex, RFC7914_EXPECTED);
    }

    #[test]
    fn aes_gcm_matches_nist_vector() {
        let key = [0_u8; 32];
        let nonce = [0_u8; 12];
        let plaintext = [0_u8; 16];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_slice())
            .unwrap();
        let hex = ciphertext
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            hex,
            "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919"
        );
    }

    #[test]
    fn vault_roundtrip_and_lock_semantics() {
        let (path, root) = temp_path("roundtrip");
        let mut vault = Vault::create(&path, "correct horse battery").unwrap();
        vault.set_credential("243131", "secret-1").unwrap();
        vault.set_shared_password("shared-secret").unwrap();
        assert!(vault.has("243131"));
        assert_eq!(vault.credential("243131"), Some("secret-1"));
        assert_eq!(vault.shared_password(), Some("shared-secret"));
        let key_b64 = vault.derived_key_b64().unwrap();
        vault.lock();
        assert!(!vault.has("243131"));
        assert_eq!(vault.credential("243131"), None);
        assert_eq!(vault.shared_password(), None);
        assert!(matches!(
            vault.set_credential("243131", "x"),
            Err(VaultError::Locked)
        ));

        let mut reopened = Vault::open(&path).unwrap().unwrap();
        assert!(reopened.unlock("correct horse battery").is_ok());
        assert_eq!(reopened.credential("243131"), Some("secret-1"));

        let mut by_key = Vault::open(&path).unwrap().unwrap();
        by_key
            .unlock_with_key(&B64.decode(key_b64).unwrap())
            .unwrap();
        assert_eq!(by_key.credential("243131"), Some("secret-1"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let (path, root) = temp_path("wrong");
        let mut vault = Vault::create(&path, "correct horse battery").unwrap();
        vault.set_credential("A1", "pw").unwrap();
        let mut reopened = Vault::open(&path).unwrap().unwrap();
        assert!(matches!(
            reopened.unlock("wrong passphrase"),
            Err(VaultError::WrongPassphrase)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn change_passphrase_keeps_entries() {
        let (path, root) = temp_path("change");
        let mut vault = Vault::create(&path, "old passphrase 1").unwrap();
        vault.set_credential("A1", "pw").unwrap();
        vault
            .change_passphrase("old passphrase 1", "new passphrase 2")
            .unwrap();
        assert_eq!(vault.credential("A1"), Some("pw"));
        let mut reopened = Vault::open(&path).unwrap().unwrap();
        assert!(reopened.unlock("new passphrase 2").is_ok());
        drop(reopened);
        let mut old = Vault::open(&path).unwrap().unwrap();
        assert!(matches!(
            old.unlock("old passphrase 1"),
            Err(VaultError::WrongPassphrase)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_credential_moves_password() {
        let (path, root) = temp_path("rename");
        let mut vault = Vault::create(&path, "passphrase 123").unwrap();
        vault.set_credential("A1", "pw").unwrap();
        vault.rename_credential("A1", "A2").unwrap();
        assert!(!vault.has("A1"));
        assert!(vault.has("A2"));
        assert_eq!(vault.credential("A2"), Some("pw"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ciphertext_is_not_plaintext_and_no_tmp_remains() {
        let (path, root) = temp_path("cipher");
        let mut vault = Vault::create(&path, "passphrase 123").unwrap();
        vault.set_credential("A1", "top-secret-value").unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("top-secret-value"));
        assert!(!path.with_extension("enc.tmp").exists());
        assert!(raw.contains("aes-256-gcm"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn short_passphrase_is_rejected() {
        let (path, root) = temp_path("short");
        assert!(matches!(
            Vault::create(&path, "short"),
            Err(VaultError::Invalid(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
