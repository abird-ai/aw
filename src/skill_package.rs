//! Portable versioned instruction-skill bundles.
//!
//! Bundles carry UTF-8 instruction content plus deterministic integrity metadata.
//! They never contain executable hooks, tool grants, provider credentials, or authority.

use aw::{config, Error, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_BUNDLE_BYTES: usize = 96 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillBundle {
    pub format_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub content: String,
    pub content_sha256: String,
}

impl SkillBundle {
    pub fn build(
        name: impl Into<String>,
        description: Option<String>,
        content: String,
    ) -> Result<Self> {
        let name = name.into();
        config::validate_skill_name(&name)?;
        validate_description(description.as_deref())?;
        validate_content(&content)?;
        let content_sha256 = sha256_hex(content.as_bytes());
        Ok(Self {
            format_version: FORMAT_VERSION,
            name,
            description,
            content,
            content_sha256,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.format_version != FORMAT_VERSION {
            return Err(Error::Config("unsupported skill bundle format_version".into()));
        }
        config::validate_skill_name(&self.name)?;
        validate_description(self.description.as_deref())?;
        validate_content(&self.content)?;
        validate_digest(&self.content_sha256)?;
        if sha256_hex(self.content.as_bytes()) != self.content_sha256 {
            return Err(Error::Denied("skill bundle content digest mismatch"));
        }
        Ok(())
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(Error::Limit("skill bundle exceeds 96 KiB"));
        }
        let bundle: Self = serde_json::from_slice(bytes)
            .map_err(|_| Error::Config("invalid or unknown-field skill bundle JSON".into()))?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(Error::Config("skill bundle source must be a regular file".into()));
        }
        let mut bytes = Vec::new();
        file.take((MAX_BUNDLE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(Error::Limit("skill bundle exceeds 96 KiB"));
        }
        Self::parse(&bytes)
    }

    pub fn encode_pretty(&self) -> Result<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|_| Error::Config("cannot serialize skill bundle".into()))
    }

    pub fn managed_path(&self, config_path: &Path) -> Result<PathBuf> {
        self.validate()?;
        let parent = config_path
            .parent()
            .ok_or_else(|| Error::Config("configuration path has no parent directory".into()))?;
        Ok(parent.join("skills").join(format!("{}.md", self.content_sha256)))
    }


}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_content(content: &str) -> Result<()> {
    if content.len() > crate::skills::MAX_SKILL_BYTES {
        return Err(Error::Limit("skill file exceeds 64 KiB"));
    }
    Ok(())
}

fn validate_description(description: Option<&str>) -> Result<()> {
    if let Some(description) = description {
        if description.len() > 512 || description.chars().any(char::is_control) {
            return Err(Error::Config(
                "skill description must be at most 512 bytes without control characters".into(),
            ));
        }
    }
    Ok(())
}

fn validate_digest(digest: &str) -> Result<()> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Config(
            "skill bundle content_sha256 must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

pub fn write_new_or_replace(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::Config("skill bundle output has no filename".into()))?;
    let mut lock_name = OsString::from(name);
    lock_name.push(".lock");
    let lock = aw_platform::open_user_metadata_lock(&path.with_file_name(lock_name))?;
    lock.lock_exclusive()?;
    if path.exists() && !replace {
        return Err(Error::Conflict(
            "refusing to overwrite existing skill bundle without --replace",
        ));
    }
    aw_platform::atomic_write_user_metadata(path, bytes)?;
    drop(lock);
    Ok(())
}

pub fn publish_managed_content(bundle: &SkillBundle, config_path: &Path) -> Result<PathBuf> {
    let path = bundle.managed_path(config_path)?;
    if path.exists() {
        let mut existing = Vec::new();
        std::fs::File::open(&path)?
            .take((crate::skills::MAX_SKILL_BYTES + 1) as u64)
            .read_to_end(&mut existing)?;
        if existing.len() > crate::skills::MAX_SKILL_BYTES
            || sha256_hex(&existing) != bundle.content_sha256
        {
            return Err(Error::Conflict(
                "managed skill content path exists with unexpected bytes",
            ));
        }
        return Ok(path);
    }
    aw_platform::atomic_write_user_metadata(&path, bundle.content.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        crate::test_support::temp_dir("skill-bundle")
    }

    #[test]
    fn bundle_round_trip_is_deterministic_and_integrity_checked() {
        let bundle = SkillBundle::build(
            "review",
            Some("Review invariants".into()),
            "check invariants".into(),
        )
        .unwrap();
        let a = bundle.encode_pretty().unwrap();
        let b = bundle.encode_pretty().unwrap();
        assert_eq!(a, b);
        let parsed = SkillBundle::parse(&a).unwrap();
        assert_eq!(parsed, bundle);

        let mut tampered = parsed.clone();
        tampered.content.push('!');
        assert!(tampered.validate().is_err());
    }

    #[test]
    fn unknown_fields_versions_and_bad_digests_fail_closed() {
        let raw = br#"{
          "format_version": 1,
          "name": "review",
          "content": "x",
          "content_sha256": "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
          "unexpected": true
        }"#;
        assert!(SkillBundle::parse(raw).is_err());

        let mut bundle = SkillBundle::build("review", None, "x".into()).unwrap();
        bundle.format_version = 2;
        assert!(bundle.validate().is_err());
        bundle.format_version = 1;
        bundle.content_sha256 = "A".repeat(64);
        assert!(bundle.validate().is_err());
    }

    #[test]
    fn no_replace_bundle_export_is_serialized() {
        let root = temp_dir();
        let output = root.join("review.awskill.json");
        let first_path = output.clone();
        let second_path = output.clone();
        let first = std::thread::spawn(move || {
            write_new_or_replace(&first_path, b"first", false)
        });
        let second = std::thread::spawn(move || {
            write_new_or_replace(&second_path, b"second", false)
        });
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        let bytes = fs::read(&output).unwrap();
        assert!(bytes == b"first" || bytes == b"second");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn managed_install_is_content_addressed_and_idempotent() {
        let root = temp_dir();
        let config = root.join("config.toml");
        let bundle = SkillBundle::build("review", None, "safe text".into()).unwrap();
        let expected = root
            .join("skills")
            .join(format!("{}.md", bundle.content_sha256));
        assert_eq!(bundle.managed_path(&config).unwrap(), expected);

        let first = publish_managed_content(&bundle, &config).unwrap();
        let second = publish_managed_content(&bundle, &config).unwrap();
        assert_eq!(first, second);
        assert_eq!(fs::read_to_string(first).unwrap(), "safe text");
        let _ = fs::remove_dir_all(root);
    }
}
