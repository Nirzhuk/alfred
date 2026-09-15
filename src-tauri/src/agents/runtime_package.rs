//! Verification, installation, activation, and rollback for Alfred-managed runtimes.
//!
//! Managed runtimes are selected by an exact id, version, and target. Package
//! verification hashes every declared file and consumes publisher-verification
//! evidence bound to that same selection. Runtime-reported version output,
//! `PATH`, self-update, and implicit version fallback are not trust inputs.

use super::capability_manifest::PackagedRuntimeMetadata;
use crate::agent_accounts::models::{AgentProductId, ManagedRuntimeId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const RUNTIME_PACKAGE_CONTRACT_VERSION: u16 = 1;

const MANAGED_RUNTIME_DIR: &str = "managed-runtimes";
const PACKAGE_DIR: &str = "packages";
const VERSIONS_DIR: &str = "versions";
const ACTIVATIONS_DIR: &str = "activations";
const INSTALLED_MANIFEST_FILE: &str = "runtime-manifest.json";
const MAX_ARTIFACTS: usize = 128;
const MAX_ACTIVATION_RECORDS: usize = 10_000;
const RETAINED_ACTIVATION_RECORDS: usize = 128;
const MAX_INSTALLED_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ACTIVATION_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RuntimePackageErrorCode {
    ManifestInvalid,
    RuntimeMismatch,
    VersionMismatch,
    UnsupportedTarget,
    PublisherVerificationMismatch,
    PublisherUnverified,
    StorageUnavailable,
    UnsafeArtifactPath,
    ArtifactMissing,
    ArtifactInvalid,
    DigestMismatch,
    LicenseMissing,
    NoticeMissing,
    ExecutablePermissionInvalid,
    InstalledManifestMissing,
    InstalledManifestInvalid,
    InstalledManifestMismatch,
    ActivationStateInvalid,
    PreviousVerificationRequired,
    ActivationCommitFailed,
    RollbackUnavailable,
}

impl RuntimePackageErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManifestInvalid => "runtime_manifest_invalid",
            Self::RuntimeMismatch => "runtime_id_mismatch",
            Self::VersionMismatch => "runtime_version_mismatch",
            Self::UnsupportedTarget => "runtime_target_unsupported",
            Self::PublisherVerificationMismatch => "runtime_publisher_verification_mismatch",
            Self::PublisherUnverified => "runtime_publisher_unverified",
            Self::StorageUnavailable => "runtime_storage_unavailable",
            Self::UnsafeArtifactPath => "runtime_artifact_path_unsafe",
            Self::ArtifactMissing => "runtime_artifact_missing",
            Self::ArtifactInvalid => "runtime_artifact_invalid",
            Self::DigestMismatch => "runtime_artifact_digest_mismatch",
            Self::LicenseMissing => "runtime_license_missing",
            Self::NoticeMissing => "runtime_notice_missing",
            Self::ExecutablePermissionInvalid => "runtime_executable_permission_invalid",
            Self::InstalledManifestMissing => "runtime_installed_manifest_missing",
            Self::InstalledManifestInvalid => "runtime_installed_manifest_invalid",
            Self::InstalledManifestMismatch => "runtime_installed_manifest_mismatch",
            Self::ActivationStateInvalid => "runtime_activation_state_invalid",
            Self::PreviousVerificationRequired => "runtime_previous_verification_required",
            Self::ActivationCommitFailed => "runtime_activation_commit_failed",
            Self::RollbackUnavailable => "runtime_rollback_unavailable",
        }
    }
}

impl fmt::Debug for RuntimePackageErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for RuntimePackageErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

pub struct RuntimePackageError {
    code: RuntimePackageErrorCode,
}

impl RuntimePackageError {
    fn new(code: RuntimePackageErrorCode) -> Self {
        Self { code }
    }

    pub fn code(&self) -> RuntimePackageErrorCode {
        self.code
    }
}

impl fmt::Debug for RuntimePackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl fmt::Display for RuntimePackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for RuntimePackageError {}

type PackageResult<T> = Result<T, RuntimePackageError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublisherVerificationScheme {
    AppleDeveloperId,
    WindowsAuthenticode,
    SigstoreBundle,
    PlatformPackageSignature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublisherVerificationStatus {
    Verified,
    Failed,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeArtifactManifest {
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeLicenseNoticeRequirements {
    pub license_expression: String,
    pub license_resource_path: String,
    pub notice_resource_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimePublisherRequirement {
    pub scheme: PublisherVerificationScheme,
    pub publisher: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeRollbackMetadata {
    pub retain_previous_verified: bool,
    pub automatic_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeUpdatePolicy {
    pub alfred_managed: bool,
    pub self_update_allowed: bool,
    pub path_lookup_allowed: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeTargetManifest {
    pub target: String,
    pub executable: RuntimeArtifactManifest,
    pub resources: Vec<RuntimeArtifactManifest>,
    pub publisher_verification: RuntimePublisherRequirement,
    pub license_notice: RuntimeLicenseNoticeRequirements,
    pub rollback: RuntimeRollbackMetadata,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimePackageManifest {
    pub schema_version: u16,
    pub contract_version: u16,
    pub runtime_id: ManagedRuntimeId,
    pub runtime_version: String,
    pub update_policy: RuntimeUpdatePolicy,
    pub targets: Vec<RuntimeTargetManifest>,
}

impl RuntimePackageManifest {
    pub fn validate(&self) -> PackageResult<()> {
        if self.schema_version != RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION
            || self.contract_version != RUNTIME_PACKAGE_CONTRACT_VERSION
            || !safe_component(&self.runtime_version, 128)
            || self.runtime_version.eq_ignore_ascii_case("latest")
            || !self.update_policy.alfred_managed
            || self.update_policy.self_update_allowed
            || self.update_policy.path_lookup_allowed
            || self.targets.is_empty()
            || self.targets.len() > 32
        {
            return Err(package_error(RuntimePackageErrorCode::ManifestInvalid));
        }
        for (index, target) in self.targets.iter().enumerate() {
            if !safe_component(&target.target, 128)
                || target.target.eq_ignore_ascii_case("latest")
                || self.targets[..index]
                    .iter()
                    .any(|candidate| candidate.target.eq_ignore_ascii_case(&target.target))
                || !valid_artifact(&target.executable)
                || target
                    .executable
                    .relative_path
                    .eq_ignore_ascii_case(INSTALLED_MANIFEST_FILE)
                || target.resources.is_empty()
                || target.resources.len() > MAX_ARTIFACTS
                || !safe_label(&target.publisher_verification.publisher, 256)
                || target
                    .publisher_verification
                    .publisher
                    .eq_ignore_ascii_case("not_applicable")
                || !target.publisher_verification.required
                || !target.rollback.retain_previous_verified
                || target.rollback.automatic_fallback
                || !safe_label(&target.license_notice.license_expression, 128)
                || target
                    .license_notice
                    .license_expression
                    .eq_ignore_ascii_case("not_applicable")
                || !safe_relative_path(&target.license_notice.license_resource_path)
                || !safe_relative_path(&target.license_notice.notice_resource_path)
                || target
                    .license_notice
                    .license_resource_path
                    .eq_ignore_ascii_case(&target.license_notice.notice_resource_path)
            {
                return Err(package_error(RuntimePackageErrorCode::ManifestInvalid));
            }
            for (resource_index, resource) in target.resources.iter().enumerate() {
                if !valid_artifact(resource)
                    || resource
                        .relative_path
                        .eq_ignore_ascii_case(INSTALLED_MANIFEST_FILE)
                    || resource
                        .relative_path
                        .eq_ignore_ascii_case(&target.executable.relative_path)
                    || target.resources[..resource_index].iter().any(|candidate| {
                        candidate
                            .relative_path
                            .eq_ignore_ascii_case(&resource.relative_path)
                    })
                {
                    return Err(package_error(RuntimePackageErrorCode::ManifestInvalid));
                }
            }
            let has_license = target.resources.iter().any(|resource| {
                resource.relative_path == target.license_notice.license_resource_path
            });
            let has_notice = target.resources.iter().any(|resource| {
                resource.relative_path == target.license_notice.notice_resource_path
            });
            if !has_license || !has_notice {
                return Err(package_error(RuntimePackageErrorCode::ManifestInvalid));
            }
        }
        Ok(())
    }

    pub fn select_target(&self, exact_target: &str) -> PackageResult<&RuntimeTargetManifest> {
        self.validate()?;
        if !safe_component(exact_target, 128) || exact_target.eq_ignore_ascii_case("latest") {
            return Err(package_error(RuntimePackageErrorCode::UnsupportedTarget));
        }
        self.targets
            .iter()
            .find(|candidate| candidate.target == exact_target)
            .ok_or_else(|| package_error(RuntimePackageErrorCode::UnsupportedTarget))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePackageExpectation {
    product: AgentProductId,
    runtime_id: ManagedRuntimeId,
    runtime_version: String,
    target: String,
}

impl RuntimePackageExpectation {
    pub fn for_product(
        product: AgentProductId,
        target: impl Into<String>,
    ) -> Result<Self, RuntimePackageError> {
        let runtime_id = product
            .managed_runtime()
            .ok_or_else(|| package_error(RuntimePackageErrorCode::RuntimeMismatch))?;
        let runtime_version = product
            .managed_runtime_version()
            .ok_or_else(|| package_error(RuntimePackageErrorCode::VersionMismatch))?;
        let expectation = Self {
            product,
            runtime_id,
            runtime_version: runtime_version.to_owned(),
            target: target.into(),
        };
        validate_expectation_pin(&expectation)?;
        Ok(expectation)
    }

    pub fn product(&self) -> AgentProductId {
        self.product
    }

    pub fn runtime_id(&self) -> ManagedRuntimeId {
        self.runtime_id
    }

    pub fn runtime_version(&self) -> &str {
        &self.runtime_version
    }

    pub fn target(&self) -> &str {
        &self.target
    }
}

#[derive(Clone, PartialEq, Eq)]
struct PublisherVerificationEvidence {
    runtime_id: ManagedRuntimeId,
    runtime_version: String,
    target: String,
    executable_sha256: String,
    scheme: PublisherVerificationScheme,
    publisher: String,
    status: PublisherVerificationStatus,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePackageVerification {
    manifest: RuntimePackageManifest,
    expectation: RuntimePackageExpectation,
    publisher: PublisherVerificationEvidence,
}

/// The result of an independent, code-owned platform publisher check.
///
/// This type is intentionally opaque.  Provider manifests and release
/// checksums are useful inputs to a verifier, but they are not themselves
/// publisher evidence and cannot construct this value.  A platform verifier
/// must create it while checking the exact package artifact against the
/// platform's signing/notarization/authenticode evidence.
pub struct RuntimePlatformPublisherEvidence {
    runtime_id: ManagedRuntimeId,
    runtime_version: String,
    target: String,
    executable_sha256: String,
    scheme: PublisherVerificationScheme,
    publisher: String,
}

impl RuntimePlatformPublisherEvidence {
    /// Constructs evidence only for code-owned platform verification. The
    /// caller must have already authenticated the artifact against the exact
    /// manifest target; the sealed verification step repeats the binding and
    /// digest checks before returning a launch capability.
    #[allow(dead_code)]
    pub(crate) fn verified(
        runtime_id: ManagedRuntimeId,
        runtime_version: impl Into<String>,
        target: impl Into<String>,
        executable_sha256: impl Into<String>,
        scheme: PublisherVerificationScheme,
        publisher: impl Into<String>,
    ) -> Self {
        Self {
            runtime_id,
            runtime_version: runtime_version.into(),
            target: target.into(),
            executable_sha256: executable_sha256.into(),
            scheme,
            publisher: publisher.into(),
        }
    }
}

impl fmt::Debug for RuntimePlatformPublisherEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimePlatformPublisherEvidence")
            .field("runtime_id", &self.runtime_id)
            .field("runtime_version", &self.runtime_version)
            .field("target", &self.target)
            .field("scheme", &self.scheme)
            .field("publisher", &self.publisher)
            .finish()
    }
}

/// Inputs supplied to a platform publisher verifier.  `publisher_proof` and
/// `platform_signature` are opaque bytes owned by the provider/platform
/// integration; this shared layer never interprets a downloaded `verified`
/// flag as proof.
pub struct RuntimePlatformPublisherRequest<'a> {
    pub package_root: &'a Path,
    pub manifest: &'a RuntimePackageManifest,
    pub expectation: &'a RuntimePackageExpectation,
    pub publisher_proof: &'a [u8],
    pub platform_signature: &'a [u8],
}

/// Code-owned platform evidence boundary.  Provider packages can request a
/// verification, but only the shared verifier implementation may return the
/// opaque evidence value used to mint [`RuntimePackageVerification`].
pub trait RuntimePlatformPublisherVerifier: Send + Sync {
    fn verify(
        &self,
        request: RuntimePlatformPublisherRequest<'_>,
    ) -> Result<RuntimePlatformPublisherEvidence, RuntimePackageErrorCode>;
}

/// Fail-closed production default until platform signing/notarization
/// verification is wired for the target package.  Keeping this explicit lets
/// product status expose a stable gate instead of claiming a package is
/// installed from descriptive metadata alone.
#[derive(Debug, Default)]
pub struct UnavailableRuntimePlatformPublisherVerifier;

impl RuntimePlatformPublisherVerifier for UnavailableRuntimePlatformPublisherVerifier {
    fn verify(
        &self,
        _request: RuntimePlatformPublisherRequest<'_>,
    ) -> Result<RuntimePlatformPublisherEvidence, RuntimePackageErrorCode> {
        Err(RuntimePackageErrorCode::PublisherUnverified)
    }
}

impl RuntimePackageVerification {
    /// The production constructor intentionally lives with the future
    /// code-owned platform verifier. Downloaded manifest JSON cannot create
    /// this capability or assert `Verified` on its own.
    #[cfg(test)]
    pub(crate) fn verified_fixture(
        manifest: RuntimePackageManifest,
        expectation: RuntimePackageExpectation,
    ) -> Result<Self, RuntimePackageError> {
        let publisher = {
            let target = manifest.select_target(expectation.target())?;
            PublisherVerificationEvidence {
                runtime_id: expectation.runtime_id,
                runtime_version: expectation.runtime_version.clone(),
                target: expectation.target.clone(),
                executable_sha256: target.executable.sha256.clone(),
                scheme: target.publisher_verification.scheme,
                publisher: target.publisher_verification.publisher.clone(),
                status: PublisherVerificationStatus::Verified,
            }
        };
        let verification = Self {
            publisher,
            manifest,
            expectation,
        };
        validate_verification(&verification)?;
        Ok(verification)
    }

    /// Mints the sealed verification capability from an independently
    /// verified platform result.  This is crate-private by design: provider
    /// modules must use a shared verifier hook and cannot assert `Verified`
    /// from release metadata or a serialized boolean.
    pub(crate) fn from_platform_evidence(
        manifest: RuntimePackageManifest,
        expectation: RuntimePackageExpectation,
        evidence: RuntimePlatformPublisherEvidence,
    ) -> PackageResult<Self> {
        let verification = Self {
            publisher: PublisherVerificationEvidence {
                runtime_id: evidence.runtime_id,
                runtime_version: evidence.runtime_version,
                target: evidence.target,
                executable_sha256: evidence.executable_sha256,
                scheme: evidence.scheme,
                publisher: evidence.publisher,
                status: PublisherVerificationStatus::Verified,
            },
            manifest,
            expectation,
        };
        validate_verification(&verification)?;
        Ok(verification)
    }

    pub fn manifest(&self) -> &RuntimePackageManifest {
        &self.manifest
    }

    pub fn expectation(&self) -> &RuntimePackageExpectation {
        &self.expectation
    }
}

/// Shared trust hook used by each provider's pinned package verifier.  The
/// provider supplies its exact manifest and publisher proof; this function
/// delegates platform evidence to the code-owned verifier, mints the sealed
/// capability, then hashes every declared package file before returning it.
pub fn verify_runtime_package_with_platform_evidence(
    package_root: &Path,
    manifest: &RuntimePackageManifest,
    expectation: &RuntimePackageExpectation,
    publisher_proof: &[u8],
    platform_signature: &[u8],
    verifier: &dyn RuntimePlatformPublisherVerifier,
) -> Result<RuntimePackageVerification, RuntimePackageError> {
    let evidence = verifier
        .verify(RuntimePlatformPublisherRequest {
            package_root,
            manifest,
            expectation,
            publisher_proof,
            platform_signature,
        })
        .map_err(package_error)?;
    let verification = RuntimePackageVerification::from_platform_evidence(
        manifest.clone(),
        expectation.clone(),
        evidence,
    )?;
    // Finish the borrowed inspection before moving the verification into the
    // result. Keeping these as separate statements avoids extending the
    // borrow through the `map` closure on older Rust borrow-checkers.
    inspect_managed_runtime_package(package_root, &verification)?;
    Ok(verification)
}

impl fmt::Debug for RuntimePackageVerification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimePackageVerification")
            .field("runtime_id", &self.expectation.runtime_id)
            .field("runtime_version", &self.expectation.runtime_version)
            .field("target", &self.expectation.target)
            .field("publisher_status", &self.publisher.status)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedRuntimePackage {
    root: PathBuf,
    expectation: RuntimePackageExpectation,
    executable_relative_path: String,
}

impl VerifiedRuntimePackage {
    pub fn executable_path(&self) -> PathBuf {
        self.root.join(&self.executable_relative_path)
    }

    pub fn expectation(&self) -> &RuntimePackageExpectation {
        &self.expectation
    }
}

impl fmt::Debug for VerifiedRuntimePackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedRuntimePackage")
            .field("runtime_id", &self.expectation.runtime_id)
            .field("runtime_version", &self.expectation.runtime_version)
            .field("target", &self.expectation.target)
            .finish()
    }
}

/// An exact package that was re-opened through the store's active activation
/// record. Unlike [`VerifiedRuntimePackage`], this is the launch capability:
/// callers cannot manufacture it from a merely inspected staging directory.
#[derive(Clone)]
pub struct RuntimePackageSelection {
    package: VerifiedRuntimePackage,
    store: RuntimePackageStore,
    verification: RuntimePackageVerification,
}

impl RuntimePackageSelection {
    pub fn expectation(&self) -> &RuntimePackageExpectation {
        self.package.expectation()
    }

    /// Rechecks both the activation record and every declared package file at
    /// the last possible boundary before process creation. A selection becomes
    /// unusable as soon as another exact version is activated.
    pub(crate) fn verified_active_executable_path(&self) -> PackageResult<PathBuf> {
        self.store
            .open_active(&self.verification)
            .map(|package| package.executable_path())
    }
}

impl fmt::Debug for RuntimePackageSelection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimePackageSelection")
            .field("runtime_id", &self.package.expectation.runtime_id)
            .field("runtime_version", &self.package.expectation.runtime_version)
            .field("target", &self.package.expectation.target)
            .finish()
    }
}

pub fn inspect_managed_runtime_package(
    package_root: &Path,
    verification: &RuntimePackageVerification,
) -> PackageResult<VerifiedRuntimePackage> {
    let target = validate_verification(verification)?;
    inspect_declared_files(package_root, &verification.expectation, target, false)
}

fn validate_verification(
    verification: &RuntimePackageVerification,
) -> PackageResult<&RuntimeTargetManifest> {
    let manifest = &verification.manifest;
    let expectation = &verification.expectation;
    manifest.validate()?;
    validate_expectation_pin(expectation)?;
    if manifest.runtime_id != expectation.runtime_id {
        return Err(package_error(RuntimePackageErrorCode::RuntimeMismatch));
    }
    if manifest.runtime_version != expectation.runtime_version {
        return Err(package_error(RuntimePackageErrorCode::VersionMismatch));
    }
    let target = manifest.select_target(&expectation.target)?;
    let publisher = &verification.publisher;
    if publisher.runtime_id != expectation.runtime_id
        || publisher.runtime_version != expectation.runtime_version
        || publisher.target != expectation.target
        || publisher.executable_sha256 != target.executable.sha256
        || publisher.scheme != target.publisher_verification.scheme
        || publisher.publisher != target.publisher_verification.publisher
    {
        return Err(package_error(
            RuntimePackageErrorCode::PublisherVerificationMismatch,
        ));
    }
    if publisher.status != PublisherVerificationStatus::Verified {
        return Err(package_error(RuntimePackageErrorCode::PublisherUnverified));
    }
    Ok(target)
}

fn validate_expectation_pin(expectation: &RuntimePackageExpectation) -> PackageResult<()> {
    if expectation.product.managed_runtime() != Some(expectation.runtime_id)
        || expectation.product.managed_runtime_version()
            != Some(expectation.runtime_version.as_str())
    {
        return Err(package_error(RuntimePackageErrorCode::RuntimeMismatch));
    }
    if !safe_component(&expectation.runtime_version, 128)
        || expectation.runtime_version.eq_ignore_ascii_case("latest")
        || !safe_component(&expectation.target, 128)
        || expectation.target.eq_ignore_ascii_case("latest")
    {
        return Err(package_error(RuntimePackageErrorCode::ManifestInvalid));
    }
    Ok(())
}

fn inspect_declared_files(
    package_root: &Path,
    expectation: &RuntimePackageExpectation,
    target: &RuntimeTargetManifest,
    allow_installed_manifest: bool,
) -> PackageResult<VerifiedRuntimePackage> {
    let root = canonical_directory(package_root)?;
    reject_undeclared_entries(&root, target, allow_installed_manifest)?;
    verify_artifact(&root, &target.executable, true)?;
    verify_required_resource(
        &root,
        &target.license_notice.license_resource_path,
        RuntimePackageErrorCode::LicenseMissing,
    )?;
    verify_required_resource(
        &root,
        &target.license_notice.notice_resource_path,
        RuntimePackageErrorCode::NoticeMissing,
    )?;
    for resource in &target.resources {
        verify_artifact(&root, resource, false)?;
    }
    Ok(VerifiedRuntimePackage {
        root,
        expectation: expectation.clone(),
        executable_relative_path: target.executable.relative_path.clone(),
    })
}

fn reject_undeclared_entries(
    root: &Path,
    target: &RuntimeTargetManifest,
    allow_installed_manifest: bool,
) -> PackageResult<()> {
    let mut allowed_files = HashSet::new();
    allowed_files.insert(target.executable.relative_path.as_str());
    for resource in &target.resources {
        allowed_files.insert(resource.relative_path.as_str());
    }
    if allow_installed_manifest {
        allowed_files.insert(INSTALLED_MANIFEST_FILE);
    }
    let mut allowed_directories = HashSet::new();
    for path in &allowed_files {
        let mut parent = Path::new(path).parent();
        while let Some(directory) = parent {
            if directory.as_os_str().is_empty() {
                break;
            }
            let directory = directory
                .to_str()
                .ok_or_else(|| package_error(RuntimePackageErrorCode::ArtifactInvalid))?;
            allowed_directories.insert(directory.replace('\\', "/"));
            parent = Path::new(directory).parent();
        }
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|_| package_error(RuntimePackageErrorCode::ArtifactInvalid))?
        {
            let entry =
                entry.map_err(|_| package_error(RuntimePackageErrorCode::ArtifactInvalid))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| package_error(RuntimePackageErrorCode::ArtifactInvalid))?;
            if metadata.file_type().is_symlink() {
                return Err(package_error(RuntimePackageErrorCode::UnsafeArtifactPath));
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .map(|value| value.replace('\\', "/"))
                .ok_or_else(|| package_error(RuntimePackageErrorCode::ArtifactInvalid))?;
            if metadata.is_dir() {
                if !allowed_directories.contains(relative.as_str()) {
                    return Err(package_error(RuntimePackageErrorCode::ArtifactInvalid));
                }
                pending.push(entry.path());
            } else if !metadata.is_file() || !allowed_files.contains(relative.as_str()) {
                return Err(package_error(RuntimePackageErrorCode::ArtifactInvalid));
            }
        }
    }
    Ok(())
}

fn verify_required_resource(
    root: &Path,
    relative_path: &str,
    missing_code: RuntimePackageErrorCode,
) -> PackageResult<()> {
    let path =
        resolve_regular_file(root, relative_path).map_err(|_| package_error(missing_code))?;
    let length = fs::metadata(path)
        .map_err(|_| package_error(missing_code))?
        .len();
    if length == 0 {
        return Err(package_error(missing_code));
    }
    Ok(())
}

fn verify_artifact(
    root: &Path,
    artifact: &RuntimeArtifactManifest,
    executable: bool,
) -> PackageResult<()> {
    let path = resolve_regular_file(root, &artifact.relative_path)?;
    if executable && !has_executable_permission(&path)? {
        return Err(package_error(
            RuntimePackageErrorCode::ExecutablePermissionInvalid,
        ));
    }
    let actual = sha256_file(&path)
        .ok_or_else(|| package_error(RuntimePackageErrorCode::ArtifactInvalid))?;
    if actual != artifact.sha256 {
        return Err(package_error(RuntimePackageErrorCode::DigestMismatch));
    }
    Ok(())
}

#[derive(Clone)]
pub struct RuntimePackageStore {
    package_root: PathBuf,
}

impl RuntimePackageStore {
    pub fn open(app_data_root: &Path) -> PackageResult<Self> {
        fs::create_dir_all(app_data_root)
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        let app_data_root = canonical_directory(app_data_root)?;
        let managed_root = app_data_root.join(MANAGED_RUNTIME_DIR);
        create_private_dir_all(&managed_root)?;
        let managed_root = canonical_directory(&managed_root)?;
        if !managed_root.starts_with(&app_data_root) {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        let package_root = managed_root.join(PACKAGE_DIR);
        create_private_dir_all(&package_root)?;
        let package_root = canonical_directory(&package_root)?;
        if !package_root.starts_with(&managed_root) {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        Ok(Self { package_root })
    }

    pub fn stage_and_activate(
        &self,
        source_root: &Path,
        verification: &RuntimePackageVerification,
        active_verification: Option<&RuntimePackageVerification>,
    ) -> PackageResult<VerifiedRuntimePackage> {
        let source = inspect_managed_runtime_package(source_root, verification)?;
        let target_root = self.target_root(&verification.expectation, true)?;
        let versions = target_root.join(VERSIONS_DIR);
        create_private_dir_all(&versions)?;
        recover_interrupted_replacements(&versions)?;
        prune_stale_staging(&versions)?;
        let destination = versions.join(&verification.expectation.runtime_version);
        let installed = if destination.exists() {
            match self.inspect_installed(&destination, verification) {
                Ok(installed) => installed,
                Err(_) => {
                    self.replace_corrupt_installed(&source, &versions, &destination, verification)?
                }
            }
        } else {
            self.stage_verified_package(&source, &versions, &destination, verification)?
        };

        let current = read_activation_record(&target_root)?;
        if current
            .as_ref()
            .is_some_and(|record| record.active_version == verification.expectation.runtime_version)
        {
            return Ok(installed);
        }
        let previous_version = match current.as_ref() {
            Some(record) => {
                let active_verification = active_verification.ok_or_else(|| {
                    package_error(RuntimePackageErrorCode::PreviousVerificationRequired)
                })?;
                if record.runtime_id != verification.expectation.runtime_id
                    || record.target != verification.expectation.target
                    || active_verification.expectation.runtime_id != record.runtime_id
                    || active_verification.expectation.runtime_version != record.active_version
                    || active_verification.expectation.target != record.target
                {
                    return Err(package_error(
                        RuntimePackageErrorCode::PreviousVerificationRequired,
                    ));
                }
                let active_root = self.installed_root(&active_verification.expectation)?;
                if self
                    .inspect_installed(&active_root, active_verification)
                    .is_ok()
                {
                    Some(record.active_version.clone())
                } else {
                    None
                }
            }
            None => None,
        };
        let generation = match current.as_ref() {
            Some(record) => record
                .generation
                .checked_add(1)
                .ok_or_else(|| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?,
            None => 1,
        };
        let record = RuntimeActivationRecord {
            schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
            generation,
            runtime_id: verification.expectation.runtime_id,
            target: verification.expectation.target.clone(),
            active_version: verification.expectation.runtime_version.clone(),
            previous_version,
        };
        write_activation_record(&target_root, &record)?;
        prune_installed_versions(
            &versions,
            &record.active_version,
            record.previous_version.as_deref(),
        )?;
        Ok(installed)
    }

    fn stage_verified_package(
        &self,
        source: &VerifiedRuntimePackage,
        versions: &Path,
        destination: &Path,
        verification: &RuntimePackageVerification,
    ) -> PackageResult<VerifiedRuntimePackage> {
        let staging = versions.join(format!(".staging-{}", uuid::Uuid::new_v4().simple()));
        create_private_dir(&staging)?;
        let stage_result = (|| {
            let target = validate_verification(verification)?;
            copy_artifact(&source.root, &staging, &target.executable, true)?;
            for resource in &target.resources {
                copy_artifact(&source.root, &staging, resource, false)?;
            }
            write_installed_manifest(&staging, &verification.manifest)?;
            sync_staged_directories(&staging)?;
            let verified = self.inspect_installed(&staging, verification)?;
            fs::rename(&staging, destination)
                .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
            sync_storage_parent(versions)?;
            Ok(VerifiedRuntimePackage {
                root: destination.to_path_buf(),
                ..verified
            })
        })();
        if stage_result.is_err() && staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        stage_result
    }

    fn replace_corrupt_installed(
        &self,
        source: &VerifiedRuntimePackage,
        versions: &Path,
        destination: &Path,
        verification: &RuntimePackageVerification,
    ) -> PackageResult<VerifiedRuntimePackage> {
        if destination.parent() != Some(versions) {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        let metadata = fs::symlink_metadata(destination)
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        let recovery = versions.join(format!(".recovery-{}", uuid::Uuid::new_v4().simple()));
        fs::rename(destination, &recovery)
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        sync_storage_parent(versions)?;
        match self.stage_verified_package(source, versions, destination, verification) {
            Ok(installed) => {
                fs::remove_dir_all(&recovery)
                    .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
                sync_storage_parent(versions)?;
                Ok(installed)
            }
            Err(error) => {
                if !destination.exists() {
                    let _ = fs::rename(&recovery, destination);
                    let _ = sync_storage_parent(versions);
                }
                Err(error)
            }
        }
    }

    pub fn open_active(
        &self,
        verification: &RuntimePackageVerification,
    ) -> PackageResult<VerifiedRuntimePackage> {
        validate_verification(verification)?;
        let target_root = self.target_root(&verification.expectation, false)?;
        let record = read_activation_record(&target_root)?
            .ok_or_else(|| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
        if record.runtime_id != verification.expectation.runtime_id
            || record.target != verification.expectation.target
            || record.active_version != verification.expectation.runtime_version
        {
            return Err(package_error(
                RuntimePackageErrorCode::ActivationStateInvalid,
            ));
        }
        let installed = self.installed_root(&verification.expectation)?;
        self.inspect_installed(&installed, verification)
    }

    /// Re-opens the exact activation selected by trusted verification input
    /// and returns the only package value accepted by the managed supervisor.
    pub fn select_active(
        &self,
        verification: &RuntimePackageVerification,
    ) -> PackageResult<RuntimePackageSelection> {
        self.open_active(verification)
            .map(|package| RuntimePackageSelection {
                package,
                store: self.clone(),
                verification: verification.clone(),
            })
    }

    pub fn rollback(
        &self,
        active_verification: &RuntimePackageVerification,
        previous_verification: &RuntimePackageVerification,
    ) -> PackageResult<VerifiedRuntimePackage> {
        validate_verification(active_verification)?;
        validate_verification(previous_verification)?;
        if active_verification.expectation.runtime_id
            != previous_verification.expectation.runtime_id
            || active_verification.expectation.target != previous_verification.expectation.target
            || active_verification.expectation.runtime_version
                == previous_verification.expectation.runtime_version
        {
            return Err(package_error(RuntimePackageErrorCode::RollbackUnavailable));
        }
        let target_root = self.target_root(&active_verification.expectation, false)?;
        let record = read_activation_record(&target_root)?
            .ok_or_else(|| package_error(RuntimePackageErrorCode::RollbackUnavailable))?;
        if record.runtime_id != active_verification.expectation.runtime_id
            || record.target != active_verification.expectation.target
            || record.active_version != active_verification.expectation.runtime_version
            || record.previous_version.as_deref()
                != Some(previous_verification.expectation.runtime_version.as_str())
        {
            return Err(package_error(RuntimePackageErrorCode::RollbackUnavailable));
        }
        let active_is_verified = self
            .installed_root(&active_verification.expectation)
            .and_then(|root| self.inspect_installed(&root, active_verification))
            .is_ok();
        let previous_root = self.installed_root(&previous_verification.expectation)?;
        let previous = self.inspect_installed(&previous_root, previous_verification)?;
        let generation = record
            .generation
            .checked_add(1)
            .ok_or_else(|| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
        let rollback_record = RuntimeActivationRecord {
            schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
            generation,
            runtime_id: record.runtime_id,
            target: record.target,
            active_version: previous_verification.expectation.runtime_version.clone(),
            previous_version: active_is_verified
                .then(|| active_verification.expectation.runtime_version.clone()),
        };
        write_activation_record(&target_root, &rollback_record)?;
        let versions = target_root.join(VERSIONS_DIR);
        prune_installed_versions(
            &versions,
            &rollback_record.active_version,
            rollback_record.previous_version.as_deref(),
        )?;
        Ok(previous)
    }

    fn target_root(
        &self,
        expectation: &RuntimePackageExpectation,
        create: bool,
    ) -> PackageResult<PathBuf> {
        if !safe_component(expectation.runtime_id.as_str(), 128)
            || !safe_component(&expectation.target, 128)
        {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        let target_root = self
            .package_root
            .join(expectation.runtime_id.as_str())
            .join(&expectation.target);
        if create {
            create_private_dir_all(&target_root)?;
        }
        let target_root = canonical_directory(&target_root)?;
        if !target_root.starts_with(&self.package_root) {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        Ok(target_root)
    }

    fn installed_root(&self, expectation: &RuntimePackageExpectation) -> PackageResult<PathBuf> {
        if !safe_component(&expectation.runtime_version, 128) {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        let target_root = self.target_root(expectation, false)?;
        let installed = target_root
            .join(VERSIONS_DIR)
            .join(&expectation.runtime_version);
        let installed = canonical_directory(&installed)?;
        if !installed.starts_with(&target_root) {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        Ok(installed)
    }

    fn inspect_installed(
        &self,
        installed_root: &Path,
        verification: &RuntimePackageVerification,
    ) -> PackageResult<VerifiedRuntimePackage> {
        let stored_manifest_path = installed_root.join(INSTALLED_MANIFEST_FILE);
        let metadata = fs::symlink_metadata(&stored_manifest_path)
            .map_err(|_| package_error(RuntimePackageErrorCode::InstalledManifestMissing))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(package_error(
                RuntimePackageErrorCode::InstalledManifestInvalid,
            ));
        }
        let bytes = read_bounded_file(
            &stored_manifest_path,
            MAX_INSTALLED_MANIFEST_BYTES,
            RuntimePackageErrorCode::InstalledManifestInvalid,
        )?;
        let stored: RuntimePackageManifest = serde_json::from_slice(&bytes)
            .map_err(|_| package_error(RuntimePackageErrorCode::InstalledManifestInvalid))?;
        if stored != verification.manifest {
            return Err(package_error(
                RuntimePackageErrorCode::InstalledManifestMismatch,
            ));
        }
        let target = validate_verification(verification)?;
        inspect_declared_files(installed_root, &verification.expectation, target, true)
    }
}

impl fmt::Debug for RuntimePackageStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RuntimePackageStore")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RuntimeActivationRecord {
    schema_version: u16,
    generation: u64,
    runtime_id: ManagedRuntimeId,
    target: String,
    active_version: String,
    previous_version: Option<String>,
}

fn write_installed_manifest(root: &Path, manifest: &RuntimePackageManifest) -> PackageResult<()> {
    let path = root.join(INSTALLED_MANIFEST_FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    serde_json::to_writer(&mut file, manifest)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    file.write_all(b"\n")
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    set_private_file_permissions(&file)?;
    file.sync_all()
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))
}

fn write_activation_record(root: &Path, record: &RuntimeActivationRecord) -> PackageResult<()> {
    let activations = root.join(ACTIVATIONS_DIR);
    create_private_dir_all(&activations)?;
    prune_activation_records(&activations, RETAINED_ACTIVATION_RECORDS)?;
    let nonce = uuid::Uuid::new_v4().simple();
    let temporary = activations.join(format!(".activation-{nonce}.tmp"));
    let final_path = activations.join(format!("activation-{:020}-{nonce}.json", record.generation));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))?;
        serde_json::to_writer(&mut file, record)
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))?;
        file.write_all(b"\n")
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))?;
        set_private_file_permissions(&file)?;
        file.sync_all()
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))?;
        fs::rename(&temporary, &final_path)
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))?;
        sync_activation_parent(&activations)?;
        prune_activation_records(&activations, RETAINED_ACTIVATION_RECORDS)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    write_result
}

fn read_activation_record(root: &Path) -> PackageResult<Option<RuntimeActivationRecord>> {
    let activations = root.join(ACTIVATIONS_DIR);
    if !activations.exists() {
        return Ok(None);
    }
    let activations = canonical_directory(&activations)?;
    if !activations.starts_with(root) {
        return Err(package_error(
            RuntimePackageErrorCode::ActivationStateInvalid,
        ));
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(&activations)
        .map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?
    {
        let entry =
            entry.map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("activation-") || !name.ends_with(".json") {
            continue;
        }
        if records.len() >= MAX_ACTIVATION_RECORDS {
            return Err(package_error(
                RuntimePackageErrorCode::ActivationStateInvalid,
            ));
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(package_error(
                RuntimePackageErrorCode::ActivationStateInvalid,
            ));
        }
        let bytes = read_bounded_file(
            &entry.path(),
            MAX_ACTIVATION_RECORD_BYTES,
            RuntimePackageErrorCode::ActivationStateInvalid,
        )?;
        let record: RuntimeActivationRecord = serde_json::from_slice(&bytes)
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
        if record_filename_generation(&name, "activation-") != Some(record.generation)
            || record.schema_version != RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION
            || record.generation == 0
            || !safe_component(record.runtime_id.as_str(), 128)
            || !safe_component(&record.target, 128)
            || !safe_component(&record.active_version, 128)
            || record
                .previous_version
                .as_deref()
                .is_some_and(|version| !safe_component(version, 128))
            || record.previous_version.as_deref() == Some(record.active_version.as_str())
        {
            return Err(package_error(
                RuntimePackageErrorCode::ActivationStateInvalid,
            ));
        }
        records.push(record);
    }
    let Some(max_generation) = records.iter().map(|record| record.generation).max() else {
        return Ok(None);
    };
    let mut current = records
        .into_iter()
        .filter(|record| record.generation == max_generation);
    let record = current
        .next()
        .ok_or_else(|| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
    if current.next().is_some() {
        return Err(package_error(
            RuntimePackageErrorCode::ActivationStateInvalid,
        ));
    }
    Ok(Some(record))
}

fn record_filename_generation(name: &str, prefix: &str) -> Option<u64> {
    let body = name.strip_prefix(prefix)?.strip_suffix(".json")?;
    let (generation, nonce) = body.split_once('-')?;
    if generation.len() != 20
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
        || nonce.len() != 32
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    generation.parse().ok()
}

fn copy_artifact(
    source_root: &Path,
    destination_root: &Path,
    artifact: &RuntimeArtifactManifest,
    executable: bool,
) -> PackageResult<()> {
    let source = resolve_regular_file(source_root, &artifact.relative_path)?;
    // A declared resource can itself be an executable the runtime must exec
    // (Codex ships the codex CLI as `libexec/codex`). Staging every resource
    // 0o600 silently breaks those runtimes at startup. Carrying the source mode
    // bit over is safe: the source root is publisher-verified, and
    // `inspect_installed` re-checks every staged digest after the copy, so only
    // the mode of already-pinned content is preserved.
    let executable = executable || source_is_executable(&source);
    let destination = destination_root.join(&artifact.relative_path);
    let parent = destination
        .parent()
        .ok_or_else(|| package_error(RuntimePackageErrorCode::UnsafeArtifactPath))?;
    create_private_dir_all(parent)?;
    fs::copy(source, &destination)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    set_artifact_permissions(&destination, executable)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&destination)
        .and_then(|file| file.sync_all())
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))
}

fn prune_stale_staging(versions: &Path) -> PackageResult<()> {
    for entry in fs::read_dir(versions)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?
    {
        let entry =
            entry.map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(".staging-") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        fs::remove_dir_all(entry.path())
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    }
    sync_storage_parent(versions)
}

fn recover_interrupted_replacements(versions: &Path) -> PackageResult<()> {
    for entry in fs::read_dir(versions)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?
    {
        let entry =
            entry.map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(".recovery-") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        let manifest_path = entry.path().join(INSTALLED_MANIFEST_FILE);
        let bytes = read_bounded_file(
            &manifest_path,
            MAX_INSTALLED_MANIFEST_BYTES,
            RuntimePackageErrorCode::InstalledManifestInvalid,
        )?;
        let manifest: RuntimePackageManifest = serde_json::from_slice(&bytes)
            .map_err(|_| package_error(RuntimePackageErrorCode::InstalledManifestInvalid))?;
        manifest.validate()?;
        let destination = versions.join(&manifest.runtime_version);
        if destination.exists() {
            fs::remove_dir_all(entry.path())
                .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        } else {
            fs::rename(entry.path(), destination)
                .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        }
    }
    sync_storage_parent(versions)
}

fn prune_installed_versions(
    versions: &Path,
    active_version: &str,
    previous_version: Option<&str>,
) -> PackageResult<()> {
    prune_stale_staging(versions)?;
    for entry in fs::read_dir(versions)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?
    {
        let entry =
            entry.map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == active_version || previous_version == Some(name.as_ref()) {
            continue;
        }
        if !safe_component(&name, 128) {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
        }
        fs::remove_dir_all(entry.path())
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    }
    sync_storage_parent(versions)
}

fn sync_staged_directories(root: &Path) -> PackageResult<()> {
    let mut directories = vec![root.to_path_buf()];
    let mut index = 0;
    while index < directories.len() {
        let directory = directories[index].clone();
        index += 1;
        for entry in fs::read_dir(&directory)
            .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?
        {
            let entry =
                entry.map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
            if metadata.file_type().is_symlink() {
                return Err(package_error(RuntimePackageErrorCode::UnsafeArtifactPath));
            }
            if metadata.is_dir() {
                directories.push(entry.path());
            }
        }
    }
    for directory in directories.into_iter().rev() {
        sync_storage_parent(&directory)?;
    }
    Ok(())
}

fn prune_activation_records(activations: &Path, retain: usize) -> PackageResult<()> {
    let mut records = Vec::new();
    let mut removed = false;
    for entry in fs::read_dir(activations)
        .map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?
    {
        let entry =
            entry.map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".activation-") && name.ends_with(".tmp") {
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(package_error(
                    RuntimePackageErrorCode::ActivationStateInvalid,
                ));
            }
            fs::remove_file(entry.path())
                .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))?;
            removed = true;
            continue;
        }
        let Some(generation) = record_filename_generation(&name, "activation-") else {
            continue;
        };
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationStateInvalid))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(package_error(
                RuntimePackageErrorCode::ActivationStateInvalid,
            ));
        }
        records.push((generation, entry.path()));
    }
    records.sort_by_key(|(generation, _)| *generation);
    let remove_count = records.len().saturating_sub(retain);
    for (_, path) in records.into_iter().take(remove_count) {
        fs::remove_file(path)
            .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))?;
        removed = true;
    }
    if removed {
        sync_activation_parent(activations)?;
    }
    Ok(())
}

fn read_bounded_file(
    path: &Path,
    max_bytes: u64,
    error_code: RuntimePackageErrorCode,
) -> PackageResult<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|_| package_error(error_code))?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(package_error(error_code));
    }
    fs::read(path).map_err(|_| package_error(error_code))
}

fn resolve_regular_file(root: &Path, relative: &str) -> PackageResult<PathBuf> {
    if !safe_relative_path(relative) {
        return Err(package_error(RuntimePackageErrorCode::UnsafeArtifactPath));
    }
    let mut candidate = root.to_path_buf();
    for segment in relative.split('/') {
        candidate.push(segment);
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                package_error(RuntimePackageErrorCode::ArtifactMissing)
            } else {
                package_error(RuntimePackageErrorCode::ArtifactInvalid)
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(package_error(RuntimePackageErrorCode::UnsafeArtifactPath));
        }
    }
    let candidate = candidate
        .canonicalize()
        .map_err(|_| package_error(RuntimePackageErrorCode::ArtifactInvalid))?;
    let metadata = fs::metadata(&candidate)
        .map_err(|_| package_error(RuntimePackageErrorCode::ArtifactInvalid))?;
    if !candidate.starts_with(root) || !metadata.is_file() {
        return Err(package_error(RuntimePackageErrorCode::ArtifactInvalid));
    }
    Ok(candidate)
}

fn canonical_directory(path: &Path) -> PackageResult<PathBuf> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(package_error(RuntimePackageErrorCode::StorageUnavailable));
    }
    path.canonicalize()
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))
}

fn valid_artifact(artifact: &RuntimeArtifactManifest) -> bool {
    safe_relative_path(&artifact.relative_path)
        && artifact.sha256.len() == 64
        && artifact
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains('\0')
        && !value.contains('\\')
        && !value.contains(':')
        && value.split('/').all(|segment| {
            !segment.is_empty() && segment != "." && segment != ".." && safe_component(segment, 128)
        })
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn safe_component(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

fn safe_label(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn has_executable_permission(path: &Path) -> PackageResult<bool> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .map_err(|_| package_error(RuntimePackageErrorCode::ArtifactInvalid))
}

#[cfg(not(unix))]
fn has_executable_permission(_path: &Path) -> PackageResult<bool> {
    Ok(true)
}

fn create_private_dir(path: &Path) -> PackageResult<()> {
    fs::create_dir(path).map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    set_private_dir_permissions(path)
}

fn create_private_dir_all(path: &Path) -> PackageResult<()> {
    fs::create_dir_all(path)
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))?;
    set_private_dir_permissions(path)
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> PackageResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> PackageResult<()> {
    Ok(())
}

#[cfg(unix)]
fn source_is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn source_is_executable(_path: &Path) -> bool {
    false
}

#[cfg(unix)]
fn set_artifact_permissions(path: &Path, executable: bool) -> PackageResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))
}

#[cfg(not(unix))]
fn set_artifact_permissions(_path: &Path, _executable: bool) -> PackageResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(file: &File) -> PackageResult<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_file: &File) -> PackageResult<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_storage_parent(path: &Path) -> PackageResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| package_error(RuntimePackageErrorCode::StorageUnavailable))
}

#[cfg(not(unix))]
fn sync_storage_parent(_path: &Path) -> PackageResult<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_activation_parent(path: &Path) -> PackageResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| package_error(RuntimePackageErrorCode::ActivationCommitFailed))
}

#[cfg(not(unix))]
fn sync_activation_parent(_path: &Path) -> PackageResult<()> {
    Ok(())
}

fn package_error(code: RuntimePackageErrorCode) -> RuntimePackageError {
    RuntimePackageError::new(code)
}

// Compatibility projection for the existing capability manifest. New managed
// runtime installation uses `RuntimePackageManifest` above; this adapter stays
// until capability metadata consumes the same typed manifest in a later phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeResourceStatus {
    NotApplicable,
    Missing,
    InvalidPath,
    ChecksumMismatch,
    LicenseMissing,
    NoticeMissing,
    DataCoupled,
    SigningUnverified,
    RollbackUnverified,
    Unversioned,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePackageInspection {
    pub status: RuntimeResourceStatus,
    pub versioned: bool,
    pub checksum_verified: bool,
    pub license_present: bool,
    pub notice_present: bool,
    pub data_independent: bool,
    pub signing_verified: bool,
    pub rollback_verified: bool,
}

impl RuntimePackageInspection {
    pub fn missing(runtime_version: Option<&str>) -> Self {
        legacy_inspection(RuntimeResourceStatus::Missing, runtime_version, false)
    }
}

pub fn inspect_runtime_package(
    resource_root: &Path,
    metadata: &PackagedRuntimeMetadata,
    runtime_version: Option<&str>,
) -> RuntimePackageInspection {
    if metadata.kind == "not_applicable" {
        return legacy_inspection(RuntimeResourceStatus::NotApplicable, runtime_version, false);
    }
    if !runtime_version.is_some_and(valid_version) {
        return legacy_inspection(RuntimeResourceStatus::Unversioned, runtime_version, false);
    }
    let Some(relative) = metadata.resource_path.as_deref() else {
        return legacy_inspection(RuntimeResourceStatus::Missing, runtime_version, false);
    };
    if !metadata.included || !safe_relative_path(relative) {
        return legacy_inspection(RuntimeResourceStatus::InvalidPath, runtime_version, false);
    }
    let Ok(root) = resource_root.canonicalize() else {
        return legacy_inspection(RuntimeResourceStatus::Missing, runtime_version, false);
    };
    let Ok(runtime_path) = root.join(relative).canonicalize() else {
        return legacy_inspection(RuntimeResourceStatus::Missing, runtime_version, false);
    };
    if !runtime_path.starts_with(&root) || !runtime_path.is_file() {
        return legacy_inspection(RuntimeResourceStatus::InvalidPath, runtime_version, false);
    }
    let checksum_verified = metadata.sha256.as_deref().is_some_and(|expected| {
        expected.len() == 64
            && expected.bytes().all(|byte| byte.is_ascii_hexdigit())
            && sha256_file(&runtime_path)
                .is_some_and(|actual| actual == expected.to_ascii_lowercase())
    });
    if !checksum_verified {
        return legacy_inspection(
            RuntimeResourceStatus::ChecksumMismatch,
            runtime_version,
            false,
        );
    }
    let license_present = !metadata.license.is_empty()
        && metadata.license != "not_applicable"
        && legacy_resource_exists(&root, metadata.license_resource_path.as_deref());
    if !license_present {
        return legacy_inspection(RuntimeResourceStatus::LicenseMissing, runtime_version, true);
    }
    let notice_present = legacy_resource_exists(&root, metadata.notice_resource_path.as_deref());
    if !notice_present {
        return legacy_inspection(RuntimeResourceStatus::NoticeMissing, runtime_version, true);
    }
    if !metadata.data_independent {
        return legacy_inspection(RuntimeResourceStatus::DataCoupled, runtime_version, true);
    }
    let signing_verified = metadata.signing_status == "verified"
        && legacy_resource_exists(&root, metadata.signing_resource_path.as_deref());
    if !signing_verified {
        return legacy_inspection(
            RuntimeResourceStatus::SigningUnverified,
            runtime_version,
            true,
        );
    }
    let rollback_verified = metadata.rollback_status == "verified"
        && !metadata.automatic_fallback
        && legacy_resource_exists(&root, metadata.rollback_resource_path.as_deref());
    if !rollback_verified {
        return legacy_inspection(
            RuntimeResourceStatus::RollbackUnverified,
            runtime_version,
            true,
        );
    }
    RuntimePackageInspection {
        status: RuntimeResourceStatus::Ready,
        versioned: runtime_version.is_some_and(valid_version),
        checksum_verified: true,
        license_present: true,
        notice_present: true,
        data_independent: true,
        signing_verified: true,
        rollback_verified: true,
    }
}

fn legacy_inspection(
    status: RuntimeResourceStatus,
    runtime_version: Option<&str>,
    checksum_verified: bool,
) -> RuntimePackageInspection {
    RuntimePackageInspection {
        status,
        versioned: runtime_version.is_some_and(valid_version),
        checksum_verified,
        license_present: false,
        notice_present: false,
        data_independent: false,
        signing_verified: false,
        rollback_verified: false,
    }
}

fn legacy_resource_exists(root: &Path, relative: Option<&str>) -> bool {
    let Some(relative) = relative else {
        return false;
    };
    if !safe_relative_path(relative) {
        return false;
    }
    root.join(relative)
        .canonicalize()
        .is_ok_and(|path| path.starts_with(root) && path.is_file())
}

fn valid_version(value: &str) -> bool {
    safe_component(value, 128) && !value.eq_ignore_ascii_case("latest")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX_VERSION: &str = "0.147.0";

    fn fixture_metadata() -> PackagedRuntimeMetadata {
        PackagedRuntimeMetadata {
            kind: "fixture_sidecar".into(),
            included: true,
            resource_path: Some("runtime/fixture-runtime.txt".into()),
            checksum_status: "verified".into(),
            sha256: Some("cbf38a1c6d10d43e8451d55fc441d491c9055a2ca5d386e1dc467e63cce2e289".into()),
            license: "MIT".into(),
            license_resource_path: Some("runtime/LICENSE.txt".into()),
            notice_resource_path: Some("runtime/NOTICE.txt".into()),
            signing_resource_path: Some("runtime/SIGNING.txt".into()),
            rollback_resource_path: Some("runtime/ROLLBACK.json".into()),
            signing_status: "verified".into(),
            rollback_status: "verified".into(),
            data_independent: true,
            automatic_fallback: false,
        }
    }

    fn package_fixture(
        version: &str,
        executable_bytes: &[u8],
    ) -> (PathBuf, RuntimePackageVerification) {
        let root = std::env::temp_dir().join(format!(
            "alfred-runtime-package-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("legal")).unwrap();
        fs::write(root.join("bin/runtime"), executable_bytes).unwrap();
        fs::write(root.join("legal/LICENSE.txt"), b"fixture license").unwrap();
        fs::write(root.join("legal/NOTICE.txt"), b"fixture notice").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join("bin/runtime"), fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let target = "x86_64-fixture-os";
        let manifest = RuntimePackageManifest {
            schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
            contract_version: RUNTIME_PACKAGE_CONTRACT_VERSION,
            runtime_id: ManagedRuntimeId::CodexPythonSdk,
            runtime_version: version.into(),
            update_policy: RuntimeUpdatePolicy {
                alfred_managed: true,
                self_update_allowed: false,
                path_lookup_allowed: false,
            },
            targets: vec![RuntimeTargetManifest {
                target: target.into(),
                executable: RuntimeArtifactManifest {
                    relative_path: "bin/runtime".into(),
                    sha256: sha256_file(&root.join("bin/runtime")).unwrap(),
                },
                resources: vec![
                    RuntimeArtifactManifest {
                        relative_path: "legal/LICENSE.txt".into(),
                        sha256: sha256_file(&root.join("legal/LICENSE.txt")).unwrap(),
                    },
                    RuntimeArtifactManifest {
                        relative_path: "legal/NOTICE.txt".into(),
                        sha256: sha256_file(&root.join("legal/NOTICE.txt")).unwrap(),
                    },
                ],
                publisher_verification: RuntimePublisherRequirement {
                    scheme: PublisherVerificationScheme::PlatformPackageSignature,
                    publisher: "fixture-publisher".into(),
                    required: true,
                },
                license_notice: RuntimeLicenseNoticeRequirements {
                    license_expression: "Apache-2.0".into(),
                    license_resource_path: "legal/LICENSE.txt".into(),
                    notice_resource_path: "legal/NOTICE.txt".into(),
                },
                rollback: RuntimeRollbackMetadata {
                    retain_previous_verified: true,
                    automatic_fallback: false,
                },
            }],
        };
        let expectation =
            RuntimePackageExpectation::for_product(AgentProductId::ChatgptCodex, target).unwrap();
        (
            root,
            RuntimePackageVerification::verified_fixture(manifest, expectation).unwrap(),
        )
    }

    #[test]
    fn managed_manifest_selects_only_an_exact_target_version_and_publisher() {
        let (root, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        inspect_managed_runtime_package(&root, &verification).unwrap();

        let mut wrong_target = verification.clone();
        wrong_target.expectation.target = "aarch64-fixture-os".into();
        assert_eq!(
            inspect_managed_runtime_package(&root, &wrong_target)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::UnsupportedTarget
        );
        let mut wrong_version = verification.clone();
        wrong_version.expectation.runtime_version = "0.146.0".into();
        assert_eq!(
            inspect_managed_runtime_package(&root, &wrong_version)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::RuntimeMismatch
        );
        let mut unverified = verification.clone();
        unverified.publisher.status = PublisherVerificationStatus::Unavailable;
        assert_eq!(
            inspect_managed_runtime_package(&root, &unverified)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::PublisherUnverified
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn traversal_symlink_missing_corrupt_and_digest_mismatch_fail_closed() {
        let (root, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        let mut traversal = verification.clone();
        traversal.manifest.targets[0].executable.relative_path = "../escape".into();
        assert_eq!(
            inspect_managed_runtime_package(&root, &traversal)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::ManifestInvalid
        );

        let executable = root.join("bin/runtime");
        let executable_bytes = fs::read(&executable).unwrap();
        let executable_permissions = fs::metadata(&executable).unwrap().permissions();
        fs::remove_file(&executable).unwrap();
        assert_eq!(
            inspect_managed_runtime_package(&root, &verification)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::ArtifactMissing
        );
        fs::write(&executable, executable_bytes).unwrap();
        fs::set_permissions(&executable, executable_permissions).unwrap();

        let mut mismatch = verification.clone();
        mismatch.manifest.targets[0].executable.sha256 = "0".repeat(64);
        assert_eq!(
            inspect_managed_runtime_package(&root, &mismatch)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::PublisherVerificationMismatch
        );

        fs::write(root.join("bin/runtime"), b"corrupt runtime").unwrap();
        assert_eq!(
            inspect_managed_runtime_package(&root, &verification)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::DigestMismatch
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            fs::remove_file(root.join("bin/runtime")).unwrap();
            let outside = root.with_extension("outside");
            fs::write(&outside, b"runtime v1").unwrap();
            symlink(&outside, root.join("bin/runtime")).unwrap();
            assert_eq!(
                inspect_managed_runtime_package(&root, &verification)
                    .unwrap_err()
                    .code(),
                RuntimePackageErrorCode::UnsafeArtifactPath
            );
            fs::remove_file(outside).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dot_components_and_undeclared_package_files_fail_closed() {
        let (root, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        for component in [".", ".."] {
            let mut invalid = verification.clone();
            invalid.expectation.target = component.into();
            assert!(inspect_managed_runtime_package(&root, &invalid).is_err());
        }
        fs::write(root.join("undeclared.dll"), b"not declared").unwrap();
        assert_eq!(
            inspect_managed_runtime_package(&root, &verification)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::ArtifactInvalid
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_notice_and_corrupt_installed_manifest_fail_closed() {
        let (source, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        fs::remove_file(source.join("legal/LICENSE.txt")).unwrap();
        assert_eq!(
            inspect_managed_runtime_package(&source, &verification)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::LicenseMissing
        );
        fs::remove_dir_all(source).unwrap();

        let (source, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        fs::remove_file(source.join("legal/NOTICE.txt")).unwrap();
        assert_eq!(
            inspect_managed_runtime_package(&source, &verification)
                .unwrap_err()
                .code(),
            RuntimePackageErrorCode::NoticeMissing
        );
        fs::remove_dir_all(source).unwrap();

        let (source, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        let app_data = std::env::temp_dir().join(format!(
            "alfred-runtime-store-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = RuntimePackageStore::open(&app_data).unwrap();
        store
            .stage_and_activate(&source, &verification, None)
            .unwrap();
        let installed = store.installed_root(verification.expectation()).unwrap();
        fs::write(installed.join(INSTALLED_MANIFEST_FILE), b"not-json").unwrap();
        assert_eq!(
            store.open_active(&verification).unwrap_err().code(),
            RuntimePackageErrorCode::InstalledManifestInvalid
        );
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(app_data).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_declared_resource_stays_executable_after_staging() {
        use std::os::unix::fs::PermissionsExt;

        let (source, mut verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        // Codex ships the codex CLI as a declared resource under libexec/, not
        // as the package executable. Staging it 0o600 breaks the sidecar.
        fs::create_dir_all(source.join("libexec")).unwrap();
        fs::write(source.join("libexec/tool"), b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(
            source.join("libexec/tool"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        verification.manifest.targets[0]
            .resources
            .push(RuntimeArtifactManifest {
                relative_path: "libexec/tool".into(),
                sha256: sha256_file(&source.join("libexec/tool")).unwrap(),
            });

        let app_data = std::env::temp_dir().join(format!(
            "alfred-runtime-execbit-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = RuntimePackageStore::open(&app_data).unwrap();
        store
            .stage_and_activate(&source, &verification, None)
            .unwrap();
        let installed = store.installed_root(verification.expectation()).unwrap();

        let staged_tool = fs::metadata(installed.join("libexec/tool")).unwrap();
        assert!(
            staged_tool.permissions().mode() & 0o100 != 0,
            "executable resource lost its exec bit during staging"
        );
        let staged_license = fs::metadata(installed.join("legal/LICENSE.txt")).unwrap();
        assert_eq!(
            staged_license.permissions().mode() & 0o111,
            0,
            "a non-executable resource must not gain an exec bit"
        );

        fs::remove_dir_all(&source).ok();
        fs::remove_dir_all(&app_data).ok();
    }

    #[test]
    fn corrupt_active_package_is_restaged_and_stale_staging_is_pruned() {
        let (source, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        let app_data = std::env::temp_dir().join(format!(
            "alfred-runtime-recovery-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = RuntimePackageStore::open(&app_data).unwrap();
        store
            .stage_and_activate(&source, &verification, None)
            .unwrap();
        let installed = store.installed_root(verification.expectation()).unwrap();
        fs::write(installed.join("bin/runtime"), b"truncated").unwrap();
        let versions = installed.parent().unwrap().to_path_buf();
        let stale = versions.join(".staging-crashed");
        fs::create_dir(&stale).unwrap();
        fs::write(stale.join("partial"), b"partial").unwrap();
        store
            .stage_and_activate(&source, &verification, None)
            .unwrap();
        assert!(store.open_active(&verification).is_ok());
        assert!(!stale.exists());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn activation_history_is_pruned_without_losing_the_latest_record() {
        let (source, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        let app_data = std::env::temp_dir().join(format!(
            "alfred-runtime-history-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = RuntimePackageStore::open(&app_data).unwrap();
        store
            .stage_and_activate(&source, &verification, None)
            .unwrap();
        let target_root = store
            .target_root(verification.expectation(), false)
            .unwrap();
        for generation in 2..=(RETAINED_ACTIVATION_RECORDS as u64 + 12) {
            write_activation_record(
                &target_root,
                &RuntimeActivationRecord {
                    schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
                    generation,
                    runtime_id: verification.expectation().runtime_id(),
                    target: verification.expectation().target().to_owned(),
                    active_version: CODEX_VERSION.into(),
                    previous_version: None,
                },
            )
            .unwrap();
        }
        let activations = target_root.join(ACTIVATIONS_DIR);
        let count = fs::read_dir(activations)
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert!(count <= RETAINED_ACTIVATION_RECORDS);
        assert_eq!(
            read_activation_record(&target_root)
                .unwrap()
                .unwrap()
                .generation,
            RETAINED_ACTIVATION_RECORDS as u64 + 12
        );
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn public_debug_and_error_values_do_not_expose_paths() {
        let (root, verification) = package_fixture(CODEX_VERSION, b"runtime v1");
        let package = inspect_managed_runtime_package(&root, &verification).unwrap();
        assert!(!format!("{package:?}").contains(root.to_string_lossy().as_ref()));
        let error = package_error(RuntimePackageErrorCode::ArtifactMissing);
        assert_eq!(format!("{error:?}"), "runtime_artifact_missing");
        assert_eq!(format!("{error}"), "runtime_artifact_missing");
        assert_eq!(
            serde_json::to_string(&error.code()).unwrap(),
            "\"runtime_artifact_missing\""
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_fixture_package_still_drives_capability_inspection() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/native-release");
        let report = inspect_runtime_package(&root, &fixture_metadata(), Some("fixture-1.0.0"));
        assert_eq!(report.status, RuntimeResourceStatus::Ready);
        assert!(report.versioned);
        assert!(report.checksum_verified);
        assert!(report.license_present);
        assert!(report.notice_present);
        assert!(report.data_independent);
        assert!(report.signing_verified);
        assert!(report.rollback_verified);
    }

    #[test]
    fn legacy_traversal_tampering_and_missing_notices_fail_closed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/native-release");
        let mut traversal = fixture_metadata();
        traversal.resource_path = Some("../Cargo.toml".into());
        assert_eq!(
            inspect_runtime_package(&root, &traversal, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::InvalidPath
        );
        let mut missing = fixture_metadata();
        missing.resource_path = Some("runtime/NO-RUNTIME".into());
        assert_eq!(
            inspect_runtime_package(&root, &missing, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::Missing
        );
        let mut tampered = fixture_metadata();
        tampered.sha256 = Some("0".repeat(64));
        assert_eq!(
            inspect_runtime_package(&root, &tampered, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::ChecksumMismatch
        );
        let mut no_notice = fixture_metadata();
        no_notice.notice_resource_path = Some("runtime/NO-NOTICE".into());
        assert_eq!(
            inspect_runtime_package(&root, &no_notice, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::NoticeMissing
        );
        let mut no_license = fixture_metadata();
        no_license.license_resource_path = Some("runtime/NO-LICENSE".into());
        assert_eq!(
            inspect_runtime_package(&root, &no_license, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::LicenseMissing
        );
        let mut undeclared_license = fixture_metadata();
        undeclared_license.license.clear();
        assert_eq!(
            inspect_runtime_package(&root, &undeclared_license, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::LicenseMissing
        );
        let mut unsigned = fixture_metadata();
        unsigned.signing_resource_path = Some("runtime/NO-SIGNING".into());
        assert_eq!(
            inspect_runtime_package(&root, &unsigned, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::SigningUnverified
        );
        let mut no_rollback = fixture_metadata();
        no_rollback.rollback_resource_path = Some("runtime/NO-ROLLBACK".into());
        assert_eq!(
            inspect_runtime_package(&root, &no_rollback, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::RollbackUnverified
        );
        let mut automatic_fallback = fixture_metadata();
        automatic_fallback.automatic_fallback = true;
        assert_eq!(
            inspect_runtime_package(&root, &automatic_fallback, Some("fixture-1.0.0")).status,
            RuntimeResourceStatus::RollbackUnverified
        );
        assert_eq!(
            inspect_runtime_package(&root, &fixture_metadata(), None).status,
            RuntimeResourceStatus::Unversioned
        );
    }
}
