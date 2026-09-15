use super::memory_validity::artifact_content_digest;
use super::DbError;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};
use uuid::Uuid;

/// The fixed chunk size used by canonical artifact writers and bounded readers.
/// Lookup pages are smaller than one chunk, so any page crosses at most two
/// adjacent chunks and never needs an unbounded file read.
pub(crate) const ARTIFACT_CHUNK_SIZE: usize = 4096;
pub(crate) const ARTIFACT_CHUNK_DIGEST_VERSION: i64 = 1;
pub(crate) const ARTIFACT_VERIFICATION_VERIFIED: &str = "verified";
pub(crate) const ARTIFACT_VERIFICATION_LEGACY_UNVERIFIED: &str = "legacy_unverified";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArtifactVerificationState {
    Verified,
    LegacyUnverified,
}

impl ArtifactVerificationState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Verified => ARTIFACT_VERIFICATION_VERIFIED,
            Self::LegacyUnverified => ARTIFACT_VERIFICATION_LEGACY_UNVERIFIED,
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            ARTIFACT_VERIFICATION_VERIFIED => Some(Self::Verified),
            ARTIFACT_VERIFICATION_LEGACY_UNVERIFIED => Some(Self::LegacyUnverified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactChunkDigest {
    pub(crate) index: usize,
    pub(crate) digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactChunkManifest {
    pub(crate) memory_id: String,
    pub(crate) content_digest: String,
    pub(crate) digest_version: i64,
    pub(crate) byte_length: usize,
    pub(crate) chunk_size: usize,
    pub(crate) chunk_count: usize,
    pub(crate) state: ArtifactVerificationState,
    pub(crate) chunks: Vec<ArtifactChunkDigest>,
}

/// Persist a complete, writer-owned chunk manifest while the canonical body is
/// already in memory. This helper never opens or hashes the artifact path.
/// Callers must invoke it in the same transaction as the canonical memory row.
pub(crate) fn persist_artifact_chunk_manifest_conn(
    conn: &Connection,
    memory_id: &str,
    content_digest: &str,
    body: &[u8],
    created_at: &str,
) -> Result<(), DbError> {
    if memory_id.trim().is_empty() || content_digest.trim().is_empty() {
        return Err(DbError::Other("artifact_manifest_identity_missing".into()));
    }
    if artifact_content_digest(body) != content_digest {
        return Err(DbError::Other("artifact_manifest_content_mismatch".into()));
    }
    let chunks = body
        .chunks(ARTIFACT_CHUNK_SIZE)
        .map(sha256_hex)
        .collect::<Vec<_>>();
    persist_verified_manifest_conn(
        conn,
        memory_id,
        content_digest,
        body.len(),
        &chunks,
        created_at,
    )
}

fn persist_verified_manifest_conn(
    conn: &Connection,
    memory_id: &str,
    content_digest: &str,
    byte_length: usize,
    chunks: &[String],
    created_at: &str,
) -> Result<(), DbError> {
    let chunk_count_i64 = i64::try_from(chunks.len())
        .map_err(|_| DbError::Other("artifact_manifest_chunk_count_overflow".into()))?;
    let byte_length = i64::try_from(byte_length)
        .map_err(|_| DbError::Other("artifact_manifest_length_overflow".into()))?;

    conn.execute(
        "INSERT INTO artifact_chunk_manifests
           (memory_id, content_digest, digest_version, byte_length, chunk_size,
            chunk_count, verification_state, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(memory_id) DO UPDATE SET
           content_digest = excluded.content_digest,
           digest_version = excluded.digest_version,
           byte_length = excluded.byte_length,
           chunk_size = excluded.chunk_size,
           chunk_count = excluded.chunk_count,
           verification_state = excluded.verification_state,
           created_at = excluded.created_at",
        params![
            memory_id,
            content_digest,
            ARTIFACT_CHUNK_DIGEST_VERSION,
            byte_length,
            ARTIFACT_CHUNK_SIZE as i64,
            chunk_count_i64,
            ArtifactVerificationState::Verified.as_str(),
            created_at,
        ],
    )?;
    conn.execute(
        "DELETE FROM artifact_chunk_digests WHERE memory_id = ?1",
        params![memory_id],
    )?;
    for (index, digest) in chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO artifact_chunk_digests (memory_id, chunk_index, digest)
             VALUES (?1, ?2, ?3)",
            params![memory_id, index as i64, digest],
        )?;
    }
    Ok(())
}

/// Index a pre-existing artifact outside lookup reads. The file is consumed in
/// fixed-size chunks, its complete writer-compatible digest is checked against
/// the canonical identity, and no proof is published unless the root/path is
/// trusted and contains no symlink component.
pub(crate) fn index_legacy_artifact_file_conn(
    conn: &Connection,
    memory_id: &str,
    expected_content_digest: &str,
    trusted_root: &Path,
    path: &Path,
    created_at: &str,
) -> Result<bool, DbError> {
    if memory_id.trim().is_empty() || expected_content_digest.trim().is_empty() {
        return Ok(false);
    }
    let Some(path) = trusted_artifact_path(trusted_root, path) else {
        return Ok(false);
    };
    let Ok(metadata) = fs::metadata(&path) else {
        return Ok(false);
    };
    if !metadata.is_file() {
        return Ok(false);
    }
    let Ok(expected_length) = usize::try_from(metadata.len()) else {
        return Ok(false);
    };
    let Ok(mut file) = File::open(&path) else {
        return Ok(false);
    };

    let mut full_digest = Sha256::new();
    update_artifact_digest_header(&mut full_digest, expected_length);
    let mut buffer = vec![0u8; ARTIFACT_CHUNK_SIZE];
    let mut total = 0usize;
    let mut chunks = Vec::new();
    loop {
        let mut filled = 0usize;
        while filled < buffer.len() {
            match file.read(&mut buffer[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(_) => return Ok(false),
            }
        }
        if filled == 0 {
            break;
        }
        let chunk = &buffer[..filled];
        total = total.saturating_add(filled);
        full_digest.update(chunk);
        chunks.push(sha256_hex(chunk));
        if filled < buffer.len() {
            break;
        }
    }
    if total != expected_length || format!("{:x}", full_digest.finalize()) != expected_content_digest {
        return Ok(false);
    }

    persist_verified_manifest_conn(
        conn,
        memory_id,
        expected_content_digest,
        expected_length,
        &chunks,
        created_at,
    )?;
    Ok(true)
}

pub(crate) fn trusted_artifact_path(root: &Path, path: &Path) -> Option<std::path::PathBuf> {
    let canonical_root = fs::canonicalize(root).ok()?;
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    // Derive the relative suffix before walking the canonical root. macOS may
    // expose the same directory as `/var` and `/private/var`; accepting either
    // root spelling is safe because every descendant is then re-resolved
    // beneath the canonical root with symlinks rejected.
    let relative = candidate
        .strip_prefix(root)
        .or_else(|_| candidate.strip_prefix(&canonical_root))
        .ok()?;
    let mut current = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return None;
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
    }
    let canonical = current;
    if !canonical.starts_with(&canonical_root) {
        return None;
    }
    let relative = canonical.strip_prefix(&canonical_root).ok()?;
    let mut components = relative.components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || !matches!(components.next(), Some(Component::Normal(_)))
    {
        return None;
    }
    if components.next().is_some() {
        return None;
    }
    let file_name = canonical.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".txt")?;
    Uuid::parse_str(stem).ok()?;
    let metadata = fs::symlink_metadata(&canonical).ok()?;
    metadata.is_file().then_some(canonical)
}

/// Resolve a generated artifact target for a write or cleanup operation.
///
/// The final file may not exist yet, but every existing path component must be
/// a non-symlink under the canonical root. This intentionally accepts only
/// relative, normal components; callers must derive `relative` from a typed
/// storage identity rather than user input.
pub(crate) fn trusted_artifact_target(root: &Path, relative: &Path) -> Option<std::path::PathBuf> {
    if relative.is_absolute() {
        return None;
    }
    let root = fs::canonicalize(root).ok()?;
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return None;
    }
    let mut current = root.clone();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return None;
        };
        current.push(component);
        let is_final = index + 1 == components.len();
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return None;
                }
                if is_final && !metadata.is_file() {
                    return None;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && is_final => {
                let parent = current.parent()?;
                let canonical_parent = fs::canonicalize(parent).ok()?;
                if !canonical_parent.starts_with(&root) {
                    return None;
                }
                return Some(canonical_parent.join(current.file_name()?));
            }
            Err(_) => return None,
        }
    }
    let canonical = fs::canonicalize(&current).ok()?;
    canonical.starts_with(&root).then_some(canonical)
}

/// Validate an output-owned artifact without trusting any parent directory or
/// filename supplied by a legacy row. Output artifacts use the generated
/// `outputs/output-<UUID>.txt` shape beneath the trusted root.
pub(crate) fn trusted_output_artifact_path(
    root: &Path,
    path: &Path,
) -> Option<std::path::PathBuf> {
    let canonical_root = fs::canonicalize(root).ok()?;
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let relative = candidate
        .strip_prefix(root)
        .or_else(|_| candidate.strip_prefix(&canonical_root))
        .ok()?;
    let mut current = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return None;
        };
        current.push(component);
        if fs::symlink_metadata(&current)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return None;
        }
    }
    let canonical = current;
    if !canonical.starts_with(&canonical_root) {
        return None;
    }
    let mut components = canonical.strip_prefix(&canonical_root).ok()?.components();
    let Component::Normal(directory) = components.next()? else {
        return None;
    };
    if directory.to_str()? != "outputs" {
        return None;
    }
    let Component::Normal(file_name) = components.next()? else {
        return None;
    };
    if components.next().is_some() {
        return None;
    }
    let file_name = file_name.to_str()?;
    let stem = file_name.strip_prefix("output-")?.strip_suffix(".txt")?;
    Uuid::parse_str(stem).ok()?;
    fs::symlink_metadata(&canonical)
        .ok()?
        .is_file()
        .then_some(canonical)
}

fn update_artifact_digest_header(hasher: &mut Sha256, body_length: usize) {
    update_digest_field(hasher, "artifact_content_digest_version", "1");
    hasher.update(body_length.to_be_bytes());
}

fn update_digest_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.len().to_be_bytes());
    hasher.update(name.as_bytes());
    hasher.update(value.len().to_be_bytes());
    hasher.update(value.as_bytes());
}

/// Load only the adjacent chunk hashes needed for one bounded page. A caller
/// may request at most two chunks; a missing, legacy, or mismatched manifest
/// returns `None` so the service can report an explicit unavailable state.
pub(crate) fn load_artifact_chunk_manifest_range_conn(
    conn: &Connection,
    memory_id: &str,
    expected_content_digest: &str,
    first_chunk: usize,
    last_chunk: usize,
) -> Result<Option<ArtifactChunkManifest>, DbError> {
    if memory_id.trim().is_empty()
        || expected_content_digest.trim().is_empty()
        || first_chunk > last_chunk
        || last_chunk - first_chunk > 1
    {
        return Ok(None);
    }

    let row = conn
        .query_row(
            "SELECT content_digest, digest_version, byte_length, chunk_size,
                    chunk_count, verification_state
               FROM artifact_chunk_manifests
              WHERE memory_id = ?1",
            params![memory_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((content_digest, digest_version, byte_length, chunk_size, chunk_count, state)) = row
    else {
        return Ok(None);
    };
    if content_digest != expected_content_digest
        || digest_version != ARTIFACT_CHUNK_DIGEST_VERSION
        || chunk_size != ARTIFACT_CHUNK_SIZE as i64
        || byte_length < 0
        || chunk_count < 0
    {
        return Ok(None);
    }
    let Some(state) = ArtifactVerificationState::parse(&state) else {
        return Ok(None);
    };
    let byte_length = usize::try_from(byte_length)
        .map_err(|_| DbError::Other("artifact_manifest_length_invalid".into()))?;
    let chunk_count = usize::try_from(chunk_count)
        .map_err(|_| DbError::Other("artifact_manifest_chunk_count_invalid".into()))?;
    let mut chunks = Vec::new();
    if state == ArtifactVerificationState::Verified {
        if first_chunk >= chunk_count || last_chunk >= chunk_count {
            return Ok(None);
        }
        let mut statement = conn.prepare(
            "SELECT chunk_index, digest
               FROM artifact_chunk_digests
              WHERE memory_id = ?1 AND chunk_index BETWEEN ?2 AND ?3
              ORDER BY chunk_index ASC",
        )?;
        chunks = statement
            .query_map(params![memory_id, first_chunk as i64, last_chunk as i64], |row| {
                Ok(ArtifactChunkDigest {
                    index: row.get::<_, i64>(0)?.try_into().map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Integer,
                            "negative artifact chunk index".into(),
                        )
                    })?,
                    digest: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if chunks.len() != last_chunk - first_chunk + 1
            || chunks
                .iter()
                .enumerate()
                .any(|(offset, chunk)| chunk.index != first_chunk + offset)
        {
            return Ok(None);
        }
    }

    Ok(Some(ArtifactChunkManifest {
        memory_id: memory_id.to_owned(),
        content_digest,
        digest_version,
        byte_length,
        chunk_size: ARTIFACT_CHUNK_SIZE,
        chunk_count,
        state,
        chunks,
    }))
}

pub(crate) fn create_artifact_chunk_schema(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS artifact_chunk_manifests (
           memory_id TEXT PRIMARY KEY NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
           content_digest TEXT NOT NULL,
           digest_version INTEGER NOT NULL,
           byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
           chunk_size INTEGER NOT NULL CHECK (chunk_size > 0),
           chunk_count INTEGER NOT NULL CHECK (chunk_count >= 0),
           verification_state TEXT NOT NULL CHECK (
             verification_state IN ('verified', 'legacy_unverified')
           ),
           created_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS artifact_chunk_digests (
           memory_id TEXT NOT NULL REFERENCES artifact_chunk_manifests(memory_id) ON DELETE CASCADE,
           chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
           digest TEXT NOT NULL CHECK (length(digest) = 64),
           PRIMARY KEY (memory_id, chunk_index)
         );
         CREATE INDEX IF NOT EXISTS idx_artifact_chunk_digests_memory
           ON artifact_chunk_digests(memory_id, chunk_index);",
    )?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}
