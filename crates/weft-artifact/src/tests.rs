use std::fs;
use std::process::Command;
use std::sync::Arc;

use tempfile::tempdir;
use weft_domain::{ArtifactRef, BaseState, FileMode, PathOperation, RepositoryId, TreeDelta};

use crate::{ArtifactStore, ArtifactStoreError, CanonicalTreeDelta, CasDigest, FilesystemCas};

const CAS_PROCESS_ENV: &str = "WEFT_ARTIFACT_PROCESS_TEST_CAS";

fn base(object_id: &str) -> BaseState {
    BaseState::new(RepositoryId::new("repository-1").unwrap(), object_id).unwrap()
}

fn upsert(path: &str, mode: FileMode, digest: &CasDigest) -> PathOperation {
    PathOperation::Upsert {
        path: path.to_owned(),
        mode,
        blob_digest: digest.as_str().to_owned(),
    }
}

#[cfg(unix)]
fn make_writable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(not(unix))]
fn make_writable(path: &std::path::Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn cas_is_deterministic_atomic_and_safe_for_concurrent_writers() {
    let directory = tempdir().unwrap();
    let cas = Arc::new(FilesystemCas::open(directory.path()).unwrap());
    let content = b"same durable bytes\0from every writer";
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let cas = Arc::clone(&cas);
            std::thread::spawn(move || cas.put(content).unwrap())
        })
        .collect();
    let digests: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();

    assert!(digests.iter().all(|digest| digest == &digests[0]));
    assert_eq!(cas.get(&digests[0]).unwrap(), content);
    assert_eq!(
        fs::read_dir(cas.object_path(&digests[0]).parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    assert_eq!(CasDigest::parse(digests[0].as_str()).unwrap(), digests[0]);
    assert!(matches!(
        CasDigest::parse("sha256:ABC"),
        Err(ArtifactStoreError::InvalidDigest(_))
    ));
}

#[test]
fn relative_cas_root_syncs_the_current_directory_as_its_parent() {
    use std::path::Path;

    assert_eq!(
        super::cas::parent_directory(Path::new("cas")),
        Some(Path::new("."))
    );
    assert_eq!(
        super::cas::parent_directory(Path::new("state/cas")),
        Some(Path::new("state"))
    );
}

#[test]
fn cas_publication_is_safe_across_processes() {
    let directory = tempdir().unwrap();
    let cas_path = directory.path().join("cas");
    let cas = FilesystemCas::open(&cas_path).unwrap();
    let children: Vec<_> = (0..4)
        .map(|_| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "tests::cas_process_writer_helper"])
                .env(CAS_PROCESS_ENV, &cas_path)
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }

    let expected = CasDigest::for_bytes(b"cross-process canonical bytes");
    assert_eq!(
        cas.get(&expected).unwrap(),
        b"cross-process canonical bytes"
    );
    assert_eq!(
        fs::read_dir(cas.object_path(&expected).parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
#[ignore = "helper invoked by cas_publication_is_safe_across_processes"]
fn cas_process_writer_helper() {
    let Ok(path) = std::env::var(CAS_PROCESS_ENV) else {
        return;
    };
    let cas = FilesystemCas::open(path).unwrap();
    cas.put(b"cross-process canonical bytes").unwrap();
}

#[test]
fn cas_detects_corruption_missing_objects_and_size_violations() {
    let directory = tempdir().unwrap();
    let cas = FilesystemCas::with_max_object_bytes(directory.path(), 16).unwrap();
    let digest = cas.put(b"original").unwrap();
    let object_path = cas.object_path(&digest);
    make_writable(&object_path);
    fs::write(&object_path, b"tampered").unwrap();

    assert!(matches!(
        cas.get(&digest),
        Err(ArtifactStoreError::DigestMismatch { .. })
    ));
    assert!(matches!(
        cas.put(b"original"),
        Err(ArtifactStoreError::DigestMismatch { .. })
    ));
    let missing = CasDigest::for_bytes(b"missing");
    assert!(matches!(
        cas.get(&missing),
        Err(ArtifactStoreError::ObjectMissing(_))
    ));
    assert!(matches!(
        cas.put(&[0; 17]),
        Err(ArtifactStoreError::ObjectTooLarge { .. })
    ));
    let non_file = CasDigest::for_bytes(b"directory instead of object");
    fs::create_dir_all(cas.object_path(&non_file)).unwrap();
    assert!(matches!(
        cas.get(&non_file),
        Err(ArtifactStoreError::InvalidObjectType(_))
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let symlink_digest = CasDigest::for_bytes(b"symlink instead of object");
        let symlink_path = cas.object_path(&symlink_digest);
        fs::create_dir_all(symlink_path.parent().unwrap()).unwrap();
        symlink(&object_path, &symlink_path).unwrap();
        assert!(matches!(
            cas.get(&symlink_digest),
            Err(ArtifactStoreError::InvalidObjectType(_))
        ));
    }
}

#[test]
fn canonical_manifest_round_trip_is_byte_stable_and_rejects_trailing_data() {
    let blob = CasDigest::for_bytes(b"blob");
    let artifact = CanonicalTreeDelta::new(
        base("base-object-1"),
        TreeDelta::new(vec![
            PathOperation::Delete {
                path: "old.txt".to_owned(),
            },
            upsert("src/main.rs", FileMode::Executable, &blob),
        ])
        .unwrap(),
    );
    let encoded = artifact.encode().unwrap();
    let decoded = CanonicalTreeDelta::decode(&encoded).unwrap();

    assert_eq!(decoded, artifact);
    assert_eq!(decoded.encode().unwrap(), encoded);
    assert_eq!(
        CasDigest::for_bytes(&encoded).as_str(),
        "sha256:9addacc805dc7b23f39bb947a3e8d6f03fe4e184e64b2904543c9007c88c21a6"
    );

    let mut trailing = encoded;
    trailing.push(0);
    assert!(matches!(
        CanonicalTreeDelta::decode(&trailing),
        Err(ArtifactStoreError::InvalidManifest(_))
    ));
}

#[test]
fn decoder_rejects_unknown_truncated_oversized_and_noncanonical_input() {
    fn string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    let blob = CasDigest::for_bytes(b"blob");
    let artifact = CanonicalTreeDelta::new(
        base("base-object-1"),
        TreeDelta::new(vec![upsert("a.txt", FileMode::Regular, &blob)]).unwrap(),
    );
    let encoded = artifact.encode().unwrap();

    let mut unknown_version = encoded.clone();
    unknown_version[b"WEFT-ARTIFACT\0".len() + 4] = b'x';
    assert!(matches!(
        CanonicalTreeDelta::decode(&unknown_version),
        Err(ArtifactStoreError::InvalidManifest(_))
    ));
    assert!(matches!(
        CanonicalTreeDelta::decode(&encoded[..encoded.len() - 1]),
        Err(ArtifactStoreError::InvalidManifest(_))
    ));

    let mut oversized = b"WEFT-ARTIFACT\0".to_vec();
    oversized.extend_from_slice(&(16_u32 * 1024 * 1024 + 1).to_be_bytes());
    assert!(matches!(
        CanonicalTreeDelta::decode(&oversized),
        Err(ArtifactStoreError::InvalidManifest(_))
    ));

    let mut noncanonical = b"WEFT-ARTIFACT\0".to_vec();
    string(&mut noncanonical, "tree-delta-v1");
    string(&mut noncanonical, "repository-1");
    string(&mut noncanonical, "base-object-1");
    noncanonical.extend_from_slice(&2_u32.to_be_bytes());
    noncanonical.push(1);
    string(&mut noncanonical, "z.txt");
    string(&mut noncanonical, blob.as_str());
    noncanonical.push(1);
    string(&mut noncanonical, "a.txt");
    string(&mut noncanonical, blob.as_str());
    assert!(matches!(
        CanonicalTreeDelta::decode(&noncanonical),
        Err(ArtifactStoreError::DomainArtifact(_))
    ));

    let mut unknown_tag = b"WEFT-ARTIFACT\0".to_vec();
    string(&mut unknown_tag, "tree-delta-v1");
    string(&mut unknown_tag, "repository-1");
    string(&mut unknown_tag, "base-object-1");
    unknown_tag.extend_from_slice(&1_u32.to_be_bytes());
    unknown_tag.push(9);
    string(&mut unknown_tag, "a.txt");
    assert!(matches!(
        CanonicalTreeDelta::decode(&unknown_tag),
        Err(ArtifactStoreError::InvalidManifest(_))
    ));
}

#[test]
fn manifest_is_not_committed_until_every_blob_is_durable() {
    let directory = tempdir().unwrap();
    let store = ArtifactStore::open(directory.path()).unwrap();
    let missing = CasDigest::for_bytes(b"not stored");
    let artifact = CanonicalTreeDelta::new(
        base("base-object-1"),
        TreeDelta::new(vec![upsert("missing.bin", FileMode::Regular, &missing)]).unwrap(),
    );

    assert!(matches!(
        store.store_manifest(&artifact),
        Err(ArtifactStoreError::MissingReferencedBlob(_))
    ));
    let reference =
        ArtifactRef::tree_delta_v1(CasDigest::for_bytes(&artifact.encode().unwrap()).as_str())
            .unwrap();
    let manifest_digest = CasDigest::parse(reference.manifest_digest()).unwrap();
    assert!(matches!(
        store.cas().get(&manifest_digest),
        Err(ArtifactStoreError::ObjectMissing(_))
    ));
}

#[cfg(unix)]
#[test]
fn reconstructs_all_v1_file_semantics_after_provider_workspace_removal() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = tempdir().unwrap();
    let store = ArtifactStore::open(directory.path().join("artifact-store")).unwrap();
    let base_directory = directory.path().join("exact-base");
    let provider_workspace = directory.path().join("provider-workspace");
    let destination = directory.path().join("reconstructed");
    fs::create_dir(&base_directory).unwrap();
    fs::write(base_directory.join("delete.txt"), b"remove me").unwrap();
    fs::write(base_directory.join("old-name.txt"), b"rename source").unwrap();
    fs::write(base_directory.join("unchanged.txt"), b"preserved").unwrap();

    fs::create_dir(&provider_workspace).unwrap();
    let binary = b"\0binary\xffcontent\n";
    let executable = b"#!/bin/sh\nprintf 'weft\\n'\n";
    fs::write(provider_workspace.join("renamed.bin"), binary).unwrap();
    fs::write(provider_workspace.join("exec.sh"), executable).unwrap();
    let mut executable_permissions = fs::metadata(provider_workspace.join("exec.sh"))
        .unwrap()
        .permissions();
    executable_permissions.set_mode(0o755);
    fs::set_permissions(provider_workspace.join("exec.sh"), executable_permissions).unwrap();
    symlink("renamed.bin", provider_workspace.join("link-to-renamed")).unwrap();

    let binary_digest = store
        .store_blob(&fs::read(provider_workspace.join("renamed.bin")).unwrap())
        .unwrap();
    let executable_digest = store
        .store_blob(&fs::read(provider_workspace.join("exec.sh")).unwrap())
        .unwrap();
    let link_digest = store.store_blob(b"renamed.bin").unwrap();
    let exact_base = base("base-object-1");
    let artifact = CanonicalTreeDelta::new(
        exact_base.clone(),
        TreeDelta::new(vec![
            PathOperation::Delete {
                path: "delete.txt".to_owned(),
            },
            upsert("exec.sh", FileMode::Executable, &executable_digest),
            upsert("link-to-renamed", FileMode::SymbolicLink, &link_digest),
            PathOperation::Delete {
                path: "old-name.txt".to_owned(),
            },
            upsert("renamed.bin", FileMode::Regular, &binary_digest),
        ])
        .unwrap(),
    );
    let reference = store.store_manifest(&artifact).unwrap();

    fs::remove_dir_all(&provider_workspace).unwrap();
    assert!(!provider_workspace.exists());
    store
        .reconstruct(&reference, &exact_base, &base_directory, &destination)
        .unwrap();

    assert_eq!(fs::read(destination.join("renamed.bin")).unwrap(), binary);
    assert_eq!(fs::read(destination.join("exec.sh")).unwrap(), executable);
    assert_ne!(
        fs::metadata(destination.join("exec.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert_eq!(
        fs::read_link(destination.join("link-to-renamed")).unwrap(),
        std::path::PathBuf::from("renamed.bin")
    );
    assert_eq!(
        fs::read(destination.join("unchanged.txt")).unwrap(),
        b"preserved"
    );
    assert!(!destination.join("delete.txt").exists());
    assert!(!destination.join("old-name.txt").exists());
    assert_eq!(store.load_manifest(&reference).unwrap(), artifact);

    let wrong_base = base("different-base");
    let rejected_destination = directory.path().join("wrong-base-output");
    assert!(matches!(
        store.reconstruct(
            &reference,
            &wrong_base,
            &base_directory,
            &rejected_destination
        ),
        Err(ArtifactStoreError::BaseMismatch)
    ));
    assert!(!rejected_destination.exists());
}

#[test]
fn reconstruction_rejects_structurally_incompatible_exact_base() {
    let directory = tempdir().unwrap();
    let store = ArtifactStore::open(directory.path().join("store")).unwrap();
    let base_directory = directory.path().join("base");
    fs::create_dir(&base_directory).unwrap();
    let artifact = CanonicalTreeDelta::new(
        base("base-object-1"),
        TreeDelta::new(vec![PathOperation::Delete {
            path: "missing.txt".to_owned(),
        }])
        .unwrap(),
    );
    let reference = store.store_manifest(&artifact).unwrap();
    let destination = directory.path().join("destination");

    assert!(matches!(
        store.reconstruct(&reference, artifact.base(), &base_directory, &destination),
        Err(ArtifactStoreError::StructuralConflict(_))
    ));
    assert!(!destination.exists());
}

#[test]
fn corrupt_manifest_is_never_loaded_as_a_revision() {
    let directory = tempdir().unwrap();
    let store = ArtifactStore::open(directory.path()).unwrap();
    let blob = store.store_blob(b"content").unwrap();
    let artifact = CanonicalTreeDelta::new(
        base("base-object-1"),
        TreeDelta::new(vec![upsert("file.txt", FileMode::Regular, &blob)]).unwrap(),
    );
    let reference = store.store_manifest(&artifact).unwrap();
    let digest = CasDigest::parse(reference.manifest_digest()).unwrap();
    let path = store.cas().object_path(&digest);
    make_writable(&path);
    fs::write(&path, b"corrupt manifest").unwrap();

    assert!(matches!(
        store.load_manifest(&reference),
        Err(ArtifactStoreError::DigestMismatch { .. })
    ));
}

#[test]
fn invalid_domain_reference_cannot_address_the_cas() {
    assert!(ArtifactRef::tree_delta_v1("provider-only-reference").is_err());
}
