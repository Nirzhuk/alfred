//! Code-owned publisher evidence for managed runtime packages.
//!
//! OpenCode packages are Alfred-signed: a pinned Ed25519 key, a detached
//! signature over a canonical selection payload, and an evidence digest that
//! is exactly the SHA-256 of that signature.
//!
//! Claude Code on Apple targets is publisher-signed: the exact pinned
//! executable digest plus Apple Developer ID + Team ID. A GitHub archive
//! digest or `{verified:true}` JSON never mints a sealed package selection.

use super::runtime_package::{
    PublisherVerificationScheme, RuntimePackageErrorCode, RuntimePlatformPublisherEvidence,
    RuntimePlatformPublisherRequest, RuntimePlatformPublisherVerifier, RuntimeTargetManifest,
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

const OPENCODE_PUBLISHER: &str = "Alfred verified OpenCode runtime package";
const EVIDENCE_TYPE: &str = "ed25519_detached";
const MAX_PROOF_BYTES: usize = 1024 * 1024;
const SIGNATURE_BYTES: usize = 64;

pub struct AlfredEd25519PackageVerifier {
    verifying_key: VerifyingKey,
}

impl AlfredEd25519PackageVerifier {
    pub fn new(verifying_key: VerifyingKey) -> Self {
        Self { verifying_key }
    }

    pub fn from_pinned_env() -> Option<Self> {
        let hex = option_env!("ALFRED_OPENCODE_PACKAGE_VERIFYING_KEY")?;
        let key = parse_verifying_key(hex).ok()?;
        Some(Self::new(key))
    }

    pub fn from_hex(hex: &str) -> Result<Self, RuntimePackageErrorCode> {
        Ok(Self::new(parse_verifying_key(hex)?))
    }
}

impl RuntimePlatformPublisherVerifier for AlfredEd25519PackageVerifier {
    fn verify(
        &self,
        request: RuntimePlatformPublisherRequest<'_>,
    ) -> Result<RuntimePlatformPublisherEvidence, RuntimePackageErrorCode> {
        if request.publisher_proof.is_empty()
            || request.publisher_proof.len() > MAX_PROOF_BYTES
            || request.platform_signature.len() != SIGNATURE_BYTES
        {
            return Err(RuntimePackageErrorCode::PublisherUnverified);
        }
        let target = request
            .manifest
            .select_target(request.expectation.target())
            .map_err(|_| RuntimePackageErrorCode::PublisherUnverified)?;
        if target.publisher_verification.scheme
            != PublisherVerificationScheme::PlatformPackageSignature
            || target.publisher_verification.publisher != OPENCODE_PUBLISHER
            || !target.publisher_verification.required
        {
            return Err(RuntimePackageErrorCode::PublisherVerificationMismatch);
        }

        let evidence = parse_evidence_record(request.publisher_proof)?;
        let signature_digest = sha256_hex(request.platform_signature);
        if evidence.scheme != "platform_package_signature"
            || evidence.publisher != OPENCODE_PUBLISHER
            || evidence.evidence_type != EVIDENCE_TYPE
            || evidence.evidence_digest != signature_digest
        {
            return Err(RuntimePackageErrorCode::PublisherVerificationMismatch);
        }

        let payload = canonical_package_payload(
            request.expectation.runtime_id().as_str(),
            request.expectation.runtime_version(),
            request.expectation.target(),
            &target.executable.sha256,
        );
        let signature = Signature::from_slice(request.platform_signature)
            .map_err(|_| RuntimePackageErrorCode::PublisherUnverified)?;
        self.verifying_key
            .verify(payload.as_bytes(), &signature)
            .map_err(|_| RuntimePackageErrorCode::PublisherUnverified)?;

        Ok(RuntimePlatformPublisherEvidence::verified(
            request.expectation.runtime_id(),
            request.expectation.runtime_version(),
            request.expectation.target(),
            target.executable.sha256.clone(),
            PublisherVerificationScheme::PlatformPackageSignature,
            OPENCODE_PUBLISHER,
        ))
    }
}

pub fn canonical_package_payload(
    runtime_id: &str,
    runtime_version: &str,
    target: &str,
    executable_sha256: &str,
) -> String {
    format!(
        "ALFRED-OPENCODE-PACKAGE-v1\nruntime_id={runtime_id}\nruntime_version={runtime_version}\ntarget={target}\nexecutable_sha256={executable_sha256}\n"
    )
}

pub struct ProductionPackageVerifier {
    opencode: Option<AlfredEd25519PackageVerifier>,
}

impl ProductionPackageVerifier {
    pub fn new(opencode: Option<AlfredEd25519PackageVerifier>) -> Self {
        Self { opencode }
    }
}

impl RuntimePlatformPublisherVerifier for ProductionPackageVerifier {
    fn verify(
        &self,
        request: RuntimePlatformPublisherRequest<'_>,
    ) -> Result<RuntimePlatformPublisherEvidence, RuntimePackageErrorCode> {
        let target = request
            .manifest
            .select_target(request.expectation.target())
            .map_err(|_| RuntimePackageErrorCode::PublisherUnverified)?;
        if !target.publisher_verification.required {
            return Err(RuntimePackageErrorCode::PublisherUnverified);
        }
        match target.publisher_verification.scheme {
            PublisherVerificationScheme::PlatformPackageSignature => {
                let Some(opencode) = self.opencode.as_ref() else {
                    return Err(RuntimePackageErrorCode::PublisherUnverified);
                };
                opencode.verify(request)
            }
            PublisherVerificationScheme::AppleDeveloperId => {
                verify_apple_developer_id_package(request, target)
            }
            PublisherVerificationScheme::WindowsAuthenticode
            | PublisherVerificationScheme::SigstoreBundle => {
                Err(RuntimePackageErrorCode::PublisherUnverified)
            }
        }
    }
}

pub fn production_platform_verifier() -> std::sync::Arc<dyn RuntimePlatformPublisherVerifier> {
    std::sync::Arc::new(ProductionPackageVerifier::new(
        AlfredEd25519PackageVerifier::from_pinned_env(),
    ))
}

fn verify_apple_developer_id_package(
    request: RuntimePlatformPublisherRequest<'_>,
    target: &RuntimeTargetManifest,
) -> Result<RuntimePlatformPublisherEvidence, RuntimePackageErrorCode> {
    if target.publisher_verification.scheme != PublisherVerificationScheme::AppleDeveloperId
        || target.publisher_verification.publisher.is_empty()
    {
        return Err(RuntimePackageErrorCode::PublisherVerificationMismatch);
    }
    if request.publisher_proof.is_empty() || request.publisher_proof.len() > MAX_PROOF_BYTES {
        return Err(RuntimePackageErrorCode::PublisherUnverified);
    }
    let executable = request.package_root.join(&target.executable.relative_path);
    let metadata =
        fs::symlink_metadata(&executable).map_err(|_| RuntimePackageErrorCode::ArtifactMissing)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(RuntimePackageErrorCode::ArtifactInvalid);
    }
    let digest = sha256_file(&executable)?;
    if digest != target.executable.sha256 {
        return Err(RuntimePackageErrorCode::DigestMismatch);
    }
    let signed = publisher_signed_binary(request.package_root, target)?;
    verify_apple_developer_id_signature(&signed, &target.publisher_verification.publisher)?;
    Ok(RuntimePlatformPublisherEvidence::verified(
        request.expectation.runtime_id(),
        request.expectation.runtime_version(),
        request.expectation.target(),
        target.executable.sha256.clone(),
        PublisherVerificationScheme::AppleDeveloperId,
        target.publisher_verification.publisher.clone(),
    ))
}

fn publisher_signed_binary(
    package_root: &Path,
    target: &RuntimeTargetManifest,
) -> Result<std::path::PathBuf, RuntimePackageErrorCode> {
    let relative = target
        .resources
        .iter()
        .find(|resource| {
            resource.relative_path == "libexec/codex"
                || resource.relative_path == "libexec/codex.exe"
        })
        .map(|resource| resource.relative_path.as_str())
        .unwrap_or(target.executable.relative_path.as_str());
    let path = package_root.join(relative);
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| RuntimePackageErrorCode::ArtifactMissing)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(RuntimePackageErrorCode::ArtifactInvalid);
    }
    if relative != target.executable.relative_path {
        let expected = target
            .resources
            .iter()
            .find(|resource| resource.relative_path == relative)
            .ok_or(RuntimePackageErrorCode::ArtifactMissing)?;
        let digest = sha256_file(&path)?;
        if digest != expected.sha256 {
            return Err(RuntimePackageErrorCode::DigestMismatch);
        }
    }
    Ok(path)
}

fn sha256_file(path: &Path) -> Result<String, RuntimePackageErrorCode> {
    let mut file = fs::File::open(path).map_err(|_| RuntimePackageErrorCode::ArtifactMissing)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| RuntimePackageErrorCode::ArtifactInvalid)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_apple_developer_id_signature(
    executable: &Path,
    publisher: &str,
) -> Result<(), RuntimePackageErrorCode> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (executable, publisher);
        return Err(RuntimePackageErrorCode::PublisherUnverified);
    }
    #[cfg(target_os = "macos")]
    {
        let verified = Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(executable)
            .status()
            .map_err(|_| RuntimePackageErrorCode::PublisherUnverified)?;
        if !verified.success() {
            return Err(RuntimePackageErrorCode::PublisherUnverified);
        }
        let details = Command::new("/usr/bin/codesign")
            .args(["-dv", "--verbose=4"])
            .arg(executable)
            .output()
            .map_err(|_| RuntimePackageErrorCode::PublisherUnverified)?;
        let info = String::from_utf8_lossy(&details.stderr);
        let team_id = codesign_field(&info, "TeamIdentifier=");
        if team_id.is_empty() || team_id == "-" || team_id == "not set" {
            return Err(RuntimePackageErrorCode::PublisherUnverified);
        }
        if !info.contains("Authority=Developer ID Application:") {
            return Err(RuntimePackageErrorCode::PublisherUnverified);
        }
        if !info.contains(publisher) {
            return Err(RuntimePackageErrorCode::PublisherVerificationMismatch);
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn codesign_field(info: &str, prefix: &str) -> String {
    info.lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or("")
        .trim()
        .to_owned()
}

struct EvidenceRecord {
    scheme: String,
    publisher: String,
    evidence_type: String,
    evidence_digest: String,
}

fn parse_evidence_record(bytes: &[u8]) -> Result<EvidenceRecord, RuntimePackageErrorCode> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| RuntimePackageErrorCode::PublisherUnverified)?;
    let object = value
        .as_object()
        .ok_or(RuntimePackageErrorCode::PublisherUnverified)?;
    if object.contains_key("verified") {
        return Err(RuntimePackageErrorCode::PublisherUnverified);
    }
    let scheme = string_field(object, "scheme")?;
    let publisher = string_field(object, "publisher")?;
    let evidence_type = string_field(object, "evidenceType")?;
    let evidence_digest = string_field(object, "evidenceDigest")?;
    if evidence_digest.len() != 64
        || !evidence_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(RuntimePackageErrorCode::PublisherUnverified);
    }
    Ok(EvidenceRecord {
        scheme,
        publisher,
        evidence_type,
        evidence_digest,
    })
}

fn string_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, RuntimePackageErrorCode> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(RuntimePackageErrorCode::PublisherUnverified)
}

fn parse_verifying_key(hex: &str) -> Result<VerifyingKey, RuntimePackageErrorCode> {
    let bytes = decode_hex32(hex).ok_or(RuntimePackageErrorCode::PublisherUnverified)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| RuntimePackageErrorCode::PublisherUnverified)
}

fn decode_hex32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in value.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        bytes[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::models::AgentProductId;
    use crate::agents::native::providers::opencode::{package_manifest, OPENCODE_RUNTIME_VERSION};
    use crate::agents::runtime_package::RuntimePackageExpectation;
    use ed25519_dalek::{Signer, SigningKey};

    fn keypair() -> (SigningKey, AlfredEd25519PackageVerifier) {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let verifier = AlfredEd25519PackageVerifier::new(signing.verifying_key());
        (signing, verifier)
    }

    fn request<'a>(
        proof: &'a [u8],
        signature: &'a [u8],
        manifest: &'a crate::agents::runtime_package::RuntimePackageManifest,
        expectation: &'a RuntimePackageExpectation,
    ) -> RuntimePlatformPublisherRequest<'a> {
        RuntimePlatformPublisherRequest {
            package_root: Path::new("/tmp/alfred-opencode-publisher-test"),
            manifest,
            expectation,
            publisher_proof: proof,
            platform_signature: signature,
        }
    }

    fn signed_evidence(signing: &SigningKey, payload: &str) -> (Vec<u8>, Vec<u8>) {
        let signature = signing.sign(payload.as_bytes());
        let signature_bytes = signature.to_bytes().to_vec();
        let proof = serde_json::json!({
            "scheme": "platform_package_signature",
            "publisher": OPENCODE_PUBLISHER,
            "evidenceType": EVIDENCE_TYPE,
            "evidenceDigest": sha256_hex(&signature_bytes),
        })
        .to_string()
        .into_bytes();
        (proof, signature_bytes)
    }

    #[test]
    fn signed_payload_is_accepted_and_verified_flags_are_rejected() {
        let (signing, verifier) = keypair();
        let manifest = package_manifest();
        let target = manifest.targets[0].target.clone();
        let expectation =
            RuntimePackageExpectation::for_product(AgentProductId::OpencodeGo, &target)
                .expect("expectation");
        let selected = manifest
            .select_target(expectation.target())
            .expect("target");
        let payload = canonical_package_payload(
            expectation.runtime_id().as_str(),
            OPENCODE_RUNTIME_VERSION,
            expectation.target(),
            &selected.executable.sha256,
        );
        let (proof, signature) = signed_evidence(&signing, &payload);
        verifier
            .verify(request(&proof, &signature, &manifest, &expectation))
            .expect("signed evidence");

        let mut verified_flag = serde_json::from_slice::<Value>(&proof).expect("json");
        verified_flag
            .as_object_mut()
            .expect("object")
            .insert("verified".into(), Value::Bool(true));
        let tainted = serde_json::to_vec(&verified_flag).expect("tainted");
        let error = verifier
            .verify(request(&tainted, &signature, &manifest, &expectation))
            .expect_err("verified flag");
        assert_eq!(error, RuntimePackageErrorCode::PublisherUnverified);

        let wrong_signing = SigningKey::from_bytes(&[9u8; 32]);
        let (wrong_proof, wrong_signature) = signed_evidence(&wrong_signing, &payload);
        let mismatch = verifier
            .verify(request(
                &wrong_proof,
                &wrong_signature,
                &manifest,
                &expectation,
            ))
            .expect_err("wrong key");
        assert_eq!(mismatch, RuntimePackageErrorCode::PublisherUnverified);
    }

    #[test]
    fn apple_developer_id_without_a_binary_is_rejected() {
        let verifier = ProductionPackageVerifier::new(None);
        let manifest = crate::agents::native::providers::claude::package_manifest();
        let expectation = RuntimePackageExpectation::for_product(
            AgentProductId::ClaudeCodeSubscription,
            "aarch64-apple-darwin",
        )
        .expect("claude expectation");
        let error = verifier
            .verify(RuntimePlatformPublisherRequest {
                package_root: Path::new("/tmp/alfred-missing-claude-package"),
                manifest: &manifest,
                expectation: &expectation,
                publisher_proof: br#"{"version":"2.1.246"}"#,
                platform_signature: b"detached-signature",
            })
            .expect_err("missing binary");
        assert_eq!(error, RuntimePackageErrorCode::ArtifactMissing);
    }

    #[test]
    fn apple_developer_id_without_a_codex_helper_is_rejected() {
        let verifier = ProductionPackageVerifier::new(None);
        let manifest = crate::agents::native::providers::codex::package_manifest();
        let expectation = RuntimePackageExpectation::for_product(
            AgentProductId::ChatgptCodex,
            "aarch64-apple-darwin",
        )
        .expect("codex expectation");
        let error = verifier
            .verify(RuntimePlatformPublisherRequest {
                package_root: Path::new("/tmp/alfred-missing-codex-package"),
                manifest: &manifest,
                expectation: &expectation,
                publisher_proof: br#"{"bomFormat":"CycloneDX"}"#,
                platform_signature: b"cli-wheel-digest",
            })
            .expect_err("missing helper");
        assert_eq!(error, RuntimePackageErrorCode::ArtifactMissing);
    }

    #[test]
    fn production_verifier_does_not_accept_opencode_without_a_key() {
        let verifier = ProductionPackageVerifier::new(None);
        let manifest = package_manifest();
        let target = manifest.targets[0].target.clone();
        let expectation =
            RuntimePackageExpectation::for_product(AgentProductId::OpencodeGo, &target)
                .expect("expectation");
        let error = verifier
            .verify(request(b"{}", b"not-a-signature", &manifest, &expectation))
            .expect_err("missing OpenCode key");
        assert_eq!(error, RuntimePackageErrorCode::PublisherUnverified);
    }
}
