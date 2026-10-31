//! Conservative media classification for bounded complete file bytes.
//! File extensions and MIME hints are not authority. Only recognized magic
//! signatures may be offered as image/audio; even those are NOT decoded or
//! validated against a codec. Provider-native media requires a separate
//! capability, adapter and format-specific validator.

use sha2::{Digest as _, Sha256};

pub const MAX_INLINE_MEDIA_BYTES: usize = 64 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
    Resource,
}
impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Resource => "resource",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaError {
    Oversized,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDescriptor {
    pub kind: MediaKind,
    pub mime: &'static str,
    pub bytes: usize,
    pub sha256: [u8; 32],
    /// Signature identified. This is not a codec-validity/security claim.
    pub signature_identified: bool,
}
impl MediaDescriptor {
    pub fn sha256_hex(&self) -> String {
        self.sha256.iter().map(|b| format!("{b:02x}")).collect()
    }
}
pub fn inspect_complete(bytes: &[u8]) -> Result<MediaDescriptor, MediaError> {
    if bytes.len() > MAX_INLINE_MEDIA_BYTES {
        return Err(MediaError::Oversized);
    }
    let (kind, mime, matched) = classify(bytes);
    Ok(MediaDescriptor {
        kind,
        mime,
        bytes: bytes.len(),
        sha256: Sha256::digest(bytes).into(),
        signature_identified: matched,
    })
}
fn classify(data: &[u8]) -> (MediaKind, &'static str, bool) {
    let starts = |prefix: &[u8]| data.starts_with(prefix);
    let riff = |tag: &[u8]| data.len() >= 12 && starts(b"RIFF") && &data[8..12] == tag;
    let recognized = if starts(b"\x89PNG\r\n\x1a\n") {
        Some((MediaKind::Image, "image/png"))
    } else if starts(&[0xff, 0xd8, 0xff]) {
        Some((MediaKind::Image, "image/jpeg"))
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some((MediaKind::Image, "image/gif"))
    } else if riff(b"WEBP") {
        Some((MediaKind::Image, "image/webp"))
    } else if riff(b"WAVE") {
        Some((MediaKind::Audio, "audio/wav"))
    } else if starts(b"fLaC") {
        Some((MediaKind::Audio, "audio/flac"))
    } else if starts(b"ID3") {
        Some((MediaKind::Audio, "audio/mpeg"))
    } else if starts(b"OggS")
        && (data.windows(8).take(64).any(|s| s == b"OpusHead")
            || data.windows(7).take(64).any(|s| s == b"\x01vorbis"))
    {
        Some((MediaKind::Audio, "audio/ogg"))
    } else if starts(b"%PDF-") {
        Some((MediaKind::Resource, "application/pdf"))
    } else if starts(b"PK\x03\x04") {
        Some((MediaKind::Resource, "application/zip"))
    } else {
        None
    };
    match recognized {
        Some((kind, mime)) => (kind, mime, true),
        None => (MediaKind::Resource, "application/octet-stream", false),
    }
}

/// A complete, owned and bounded media packet. It is not itself an MCP
/// ContentBlock or permission to send bytes to any provider. The exact tool
/// source/grant and provider-specific media policy remain host owned.
#[derive(Debug, Clone)]
pub struct MediaPacket {
    descriptor: MediaDescriptor,
    bytes: Vec<u8>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum MediaDeliveryError<E> {
    Changed,
    NotAuthorized,
    /// Typed data may already have reached the provider before an error or
    /// panic. Optional host error is diagnostic, not replay permission.
    DeliveryUnknown(Option<E>),
}
/// Host-declared provider capability. No MIME is implicitly accepted.
#[derive(Clone, Copy, Debug)]
pub struct MediaDeliveryPolicy<'a> {
    pub accepted_mimes: &'a [&'a str],
    pub max_bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaReceipt {
    pub kind: MediaKind,
    pub mime: &'static str,
    pub bytes: usize,
    pub sha256: [u8; 32],
}
impl MediaPacket {
    pub fn from_complete_bytes(bytes: Vec<u8>) -> Result<Self, MediaError> {
        let descriptor = inspect_complete(&bytes)?;
        Ok(Self { descriptor, bytes })
    }
    pub fn descriptor(&self) -> &MediaDescriptor {
        &self.descriptor
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Validate again at the host boundary, even after a trusted Rust
    /// adapter has held the packet across async suspension.
    pub fn verify(&self) -> bool {
        inspect_complete(&self.bytes).ok().as_ref() == Some(&self.descriptor)
    }
    /// Authoritatively declared MIME and byte ceilings are rechecked before
    /// the host attempts typed delivery. A failed host attachment may be an
    /// uncertain external effect; the original journal must settle it.
    pub fn dispatch_declared<H: TypedMediaHost>(
        &self,
        host: &H,
        policy: MediaDeliveryPolicy<'_>,
    ) -> Result<MediaReceipt, MediaDeliveryError<H::Error>> {
        if !self.verify() {
            return Err(MediaDeliveryError::Changed);
        }
        let mime = self.descriptor.mime;
        if !self.descriptor.signature_identified
            || self.bytes.len() > policy.max_bytes.min(MAX_INLINE_MEDIA_BYTES)
            || !policy.accepted_mimes.contains(&mime)
        {
            return Err(MediaDeliveryError::NotAuthorized);
        }
        let permitted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.permits(self.descriptor.kind, mime, self.bytes.len())
        }))
        .unwrap_or(false);
        if !permitted {
            return Err(MediaDeliveryError::NotAuthorized);
        }
        // An attachment could have been sent before the host panicked.
        // Propagate an inspect-required outcome, not an apparent rejection.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.attach_verified(&self.descriptor, &self.bytes)
        })) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(MediaDeliveryError::DeliveryUnknown(Some(error))),
            Err(_) => return Err(MediaDeliveryError::DeliveryUnknown(None)),
        }
        Ok(MediaReceipt {
            kind: self.descriptor.kind,
            mime,
            bytes: self.bytes.len(),
            sha256: self.descriptor.sha256,
        })
    }
}

/// The producer may offer typed content only to an explicitly enabled host.
/// A MIME signature is necessary but not sufficient to authorize playback or
/// rendering: the provider's own supported format and byte budget must pass.
pub trait TypedMediaHost {
    type Error;
    fn permits(&self, kind: MediaKind, mime: &str, bytes: usize) -> bool;
    fn attach_verified(
        &self,
        description: &MediaDescriptor,
        content: &[u8],
    ) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signatures_not_extensions_select_media() {
        let image = inspect_complete(b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert_eq!(image.kind, MediaKind::Image);
        assert_eq!(image.mime, "image/png");
        assert!(image.signature_identified);
        assert_eq!(
            image.sha256_hex(),
            format!("{:x}", Sha256::digest(b"\x89PNG\r\n\x1a\nrest"))
        );
        let spoof = inspect_complete(b"i am not png").unwrap();
        assert_eq!(spoof.kind, MediaKind::Resource);
        assert_eq!(spoof.mime, "application/octet-stream");
        assert!(!spoof.signature_identified);
    }
    #[test]
    fn svg_xml_and_ambiguous_ogg_are_not_automatically_renderable() {
        let svg = inspect_complete(b"<svg onload='doStuff()'/>").unwrap();
        assert_eq!(svg.kind, MediaKind::Resource);
        let unknown = inspect_complete(b"OggS\0\0\0\0").unwrap();
        assert_eq!(unknown.kind, MediaKind::Resource);
        let audio = inspect_complete(b"OggS\x00\x00OpusHeadpayload").unwrap();
        assert_eq!(audio.mime, "audio/ogg");
    }
    struct MockHost {
        enabled: bool,
        seen: std::sync::Mutex<Vec<[u8; 32]>>,
    }
    impl TypedMediaHost for MockHost {
        type Error = &'static str;
        fn permits(&self, kind: MediaKind, mime: &str, bytes: usize) -> bool {
            self.enabled
                && kind == MediaKind::Image
                && mime == "image/png"
                && bytes <= MAX_INLINE_MEDIA_BYTES
        }
        fn attach_verified(&self, desc: &MediaDescriptor, bytes: &[u8]) -> Result<(), Self::Error> {
            if Sha256::digest(bytes).as_slice() != desc.sha256 {
                return Err("source changed");
            }
            self.seen.lock().unwrap().push(desc.sha256);
            Ok(())
        }
    }
    #[test]
    fn explicit_provider_handoff_refuses_denial_and_changed_bytes() {
        let source = b"\x89PNG\r\n\x1a\nvalid-envelope-bytes".to_vec();
        let mut packet = MediaPacket::from_complete_bytes(source).unwrap();
        let deny = MockHost {
            enabled: false,
            seen: Default::default(),
        };
        let policy = MediaDeliveryPolicy {
            accepted_mimes: &["image/png"],
            max_bytes: MAX_INLINE_MEDIA_BYTES,
        };
        assert_eq!(
            packet.dispatch_declared(&deny, policy),
            Err(MediaDeliveryError::NotAuthorized)
        );
        let allow = MockHost {
            enabled: true,
            seen: Default::default(),
        };
        packet.dispatch_declared(&allow, policy).unwrap();
        assert_eq!(allow.seen.lock().unwrap().len(), 1);
        packet.bytes[0] = 0;
        assert!(!packet.verify());
        assert_eq!(
            packet.dispatch_declared(&allow, policy),
            Err(MediaDeliveryError::Changed)
        );
        assert_eq!(allow.seen.lock().unwrap().len(), 1);
    }
    #[test]
    fn unknown_media_cannot_become_provider_image_from_filename() {
        let packet = MediaPacket::from_complete_bytes(b"<svg onclick=bad/>".to_vec()).unwrap();
        let allow = MockHost {
            enabled: true,
            seen: Default::default(),
        };
        assert_eq!(
            packet.dispatch_declared(
                &allow,
                MediaDeliveryPolicy {
                    accepted_mimes: &["application/octet-stream"],
                    max_bytes: MAX_INLINE_MEDIA_BYTES,
                }
            ),
            Err(MediaDeliveryError::NotAuthorized)
        );
        assert!(packet.verify());
        assert!(MediaPacket::from_complete_bytes(vec![0; MAX_INLINE_MEDIA_BYTES + 1]).is_err());
    }
    #[test]
    fn declared_media_contract_rejects_unsupported_mime_or_size() {
        let packet =
            MediaPacket::from_complete_bytes(b"\x89PNG\r\n\x1a\nsmall-image".to_vec()).unwrap();
        let host = MockHost {
            enabled: true,
            seen: Default::default(),
        };
        let deny_type = packet.dispatch_declared(
            &host,
            MediaDeliveryPolicy {
                accepted_mimes: &["image/jpeg"],
                max_bytes: 100,
            },
        );
        assert_eq!(deny_type, Err(MediaDeliveryError::NotAuthorized));
        let deny_size = packet.dispatch_declared(
            &host,
            MediaDeliveryPolicy {
                accepted_mimes: &["image/png"],
                max_bytes: 4,
            },
        );
        assert_eq!(deny_size, Err(MediaDeliveryError::NotAuthorized));
        assert!(host.seen.lock().unwrap().is_empty());
        let receipt = packet
            .dispatch_declared(
                &host,
                MediaDeliveryPolicy {
                    accepted_mimes: &["image/png"],
                    max_bytes: 100,
                },
            )
            .unwrap();
        assert_eq!(receipt.sha256, packet.descriptor().sha256);
        assert_eq!(receipt.mime, "image/png");
        assert_eq!(host.seen.lock().unwrap().len(), 1);
    }
    #[test]
    fn unknown_or_potentially_active_type_is_not_promoted_to_typed_media() {
        let packet = MediaPacket::from_complete_bytes(b"<svg onload=evil>".to_vec()).unwrap();
        struct Permissive;
        impl TypedMediaHost for Permissive {
            type Error = ();
            fn permits(&self, _: MediaKind, _: &str, _: usize) -> bool {
                true
            }
            fn attach_verified(&self, _: &MediaDescriptor, _: &[u8]) -> Result<(), ()> {
                Ok(())
            }
        }
        assert_eq!(
            packet.dispatch_declared(
                &Permissive,
                MediaDeliveryPolicy {
                    accepted_mimes: &["application/octet-stream"],
                    max_bytes: 100,
                }
            ),
            Err(MediaDeliveryError::NotAuthorized)
        );
    }
    #[test]
    fn panicking_host_delivery_is_unknown_and_must_not_be_retried() {
        struct Panics {
            calls: std::cell::Cell<usize>,
        }
        impl TypedMediaHost for Panics {
            type Error = ();
            fn permits(&self, _: MediaKind, _: &str, _: usize) -> bool {
                true
            }
            fn attach_verified(&self, _: &MediaDescriptor, _: &[u8]) -> Result<(), ()> {
                self.calls.set(self.calls.get() + 1);
                panic!("provider may already have received bytes");
            }
        }
        let host = Panics {
            calls: std::cell::Cell::new(0),
        };
        let packet =
            MediaPacket::from_complete_bytes(b"\x89PNG\r\n\x1a\nexample".to_vec()).unwrap();
        let result = packet.dispatch_declared(
            &host,
            MediaDeliveryPolicy {
                accepted_mimes: &["image/png"],
                max_bytes: 100,
            },
        );
        assert_eq!(result, Err(MediaDeliveryError::DeliveryUnknown(None)));
        assert_eq!(host.calls.get(), 1);
        struct Denies;
        impl TypedMediaHost for Denies {
            type Error = ();
            fn permits(&self, _: MediaKind, _: &str, _: usize) -> bool {
                panic!("authorization host panic")
            }
            fn attach_verified(&self, _: &MediaDescriptor, _: &[u8]) -> Result<(), ()> {
                panic!("must not attach without permission")
            }
        }
        assert_eq!(
            packet.dispatch_declared(
                &Denies,
                MediaDeliveryPolicy {
                    accepted_mimes: &["image/png"],
                    max_bytes: 100,
                }
            ),
            Err(MediaDeliveryError::NotAuthorized)
        );
    }
    #[test]
    fn host_error_after_attachment_remains_an_unknown_effect() {
        struct FailsAfterSend {
            delivered: std::cell::Cell<usize>,
        }
        impl TypedMediaHost for FailsAfterSend {
            type Error = &'static str;
            fn permits(&self, _: MediaKind, _: &str, _: usize) -> bool {
                true
            }
            fn attach_verified(&self, _: &MediaDescriptor, _: &[u8]) -> Result<(), Self::Error> {
                self.delivered.set(self.delivered.get() + 1);
                Err("network broke after delivery")
            }
        }
        let host = FailsAfterSend {
            delivered: std::cell::Cell::new(0),
        };
        let image =
            MediaPacket::from_complete_bytes(b"\x89PNG\r\n\x1a\nopaque-data".to_vec()).unwrap();
        let result = image.dispatch_declared(
            &host,
            MediaDeliveryPolicy {
                accepted_mimes: &["image/png"],
                max_bytes: 256,
            },
        );
        assert_eq!(
            result,
            Err(MediaDeliveryError::DeliveryUnknown(Some(
                "network broke after delivery"
            )))
        );
        assert_eq!(host.delivered.get(), 1);
    }
    #[test]
    fn only_complete_bounded_bytes_pass() {
        assert_eq!(
            inspect_complete(&vec![0u8; MAX_INLINE_MEDIA_BYTES + 1]),
            Err(MediaError::Oversized)
        );
    }
}
