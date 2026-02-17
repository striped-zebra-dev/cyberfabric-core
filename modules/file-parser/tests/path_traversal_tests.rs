#![allow(clippy::unwrap_used, clippy::expect_used, clippy::use_debug)]

use std::path::PathBuf;
use std::sync::Arc;

use file_parser::domain::error::DomainError;
use file_parser::domain::parser::FileParserBackend;
use file_parser::domain::service::{FileParserService, ServiceConfig};
use file_parser::infra::parsers::PlainTextParser;

/// Build a minimal `FileParserService` with the given base-dir restriction.
fn build_service(allowed_local_base_dir: Option<PathBuf>) -> FileParserService {
    let parsers: Vec<Arc<dyn FileParserBackend>> = vec![Arc::new(PlainTextParser::new())];
    let config = ServiceConfig {
        max_file_size_bytes: 10 * 1024 * 1024,
        allowed_local_base_dir,
    };
    FileParserService::new(parsers, config)
}

/// Create a temporary text file inside the given directory and return its path.
fn create_temp_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("failed to create temp file");
    path
}

// -----------------------------------------------------------------------
// 1. `..` component rejection (no base-dir needed)
// -----------------------------------------------------------------------

#[tokio::test]
async fn rejects_dotdot_relative_path() {
    let svc = build_service(None);
    let path = PathBuf::from("some/../../etc/passwd");

    let err = svc.parse_local(&path).await.unwrap_err();
    assert!(
        matches!(err, DomainError::PathTraversalBlocked { .. }),
        "Expected PathTraversalBlocked, got: {err:?}"
    );
}

#[tokio::test]
async fn rejects_dotdot_at_start() {
    let svc = build_service(None);
    let path = PathBuf::from("../secret.txt");

    let err = svc.parse_local(&path).await.unwrap_err();
    assert!(matches!(err, DomainError::PathTraversalBlocked { .. }));
}

#[tokio::test]
async fn rejects_dotdot_in_middle() {
    let svc = build_service(None);
    let path = PathBuf::from("/allowed/dir/../../../etc/shadow");

    let err = svc.parse_local(&path).await.unwrap_err();
    assert!(matches!(err, DomainError::PathTraversalBlocked { .. }));
}

// -----------------------------------------------------------------------
// 2. Base-dir enforcement
// -----------------------------------------------------------------------

#[tokio::test]
async fn allows_file_within_base_dir() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let base = tmp.path().canonicalize().unwrap();
    let file = create_temp_file(&base, "hello.txt", "Hello, world!");

    let svc = build_service(Some(base));
    let doc = svc.parse_local(&file).await.expect("should parse OK");
    assert!(!doc.blocks.is_empty(), "should produce blocks");
}

#[tokio::test]
async fn allows_file_in_subdirectory_of_base_dir() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let base = tmp.path().canonicalize().unwrap();
    let sub = base.join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    let file = create_temp_file(&sub, "nested.txt", "Nested content");

    let svc = build_service(Some(base));
    let doc = svc.parse_local(&file).await.expect("should parse OK");
    assert!(!doc.blocks.is_empty());
}

#[tokio::test]
async fn rejects_file_outside_base_dir() {
    let base_tmp = tempfile::tempdir().expect("failed to create base dir");
    let other_tmp = tempfile::tempdir().expect("failed to create other dir");

    let base = base_tmp.path().canonicalize().unwrap();
    let outside_file = create_temp_file(other_tmp.path(), "secret.txt", "Secret data");

    let svc = build_service(Some(base));
    let err = svc.parse_local(&outside_file).await.unwrap_err();
    assert!(
        matches!(err, DomainError::PathTraversalBlocked { .. }),
        "Expected PathTraversalBlocked, got: {err:?}"
    );
}

#[tokio::test]
async fn rejects_absolute_path_outside_base_dir() {
    let base_tmp = tempfile::tempdir().expect("failed to create base dir");
    let base = base_tmp.path().canonicalize().unwrap();

    // /etc/hostname exists on most Linux systems and has no extension,
    // but /tmp is always present — create a real file outside the base.
    let other_tmp = tempfile::tempdir().expect("failed to create other dir");
    let outside = create_temp_file(other_tmp.path(), "data.log", "log line");

    let svc = build_service(Some(base));
    let err = svc.parse_local(&outside).await.unwrap_err();
    assert!(matches!(err, DomainError::PathTraversalBlocked { .. }));
}

// -----------------------------------------------------------------------
// 3. Symlink escape prevention
// -----------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlink_escape_from_base_dir() {
    let base_tmp = tempfile::tempdir().expect("failed to create base dir");
    let external_tmp = tempfile::tempdir().expect("failed to create external dir");

    let base = base_tmp.path().canonicalize().unwrap();
    let external_file = create_temp_file(external_tmp.path(), "secret.txt", "Confidential content");

    // Create a symlink inside the base dir that points outside
    let symlink_path = base.join("escape.txt");
    std::os::unix::fs::symlink(&external_file, &symlink_path).expect("failed to create symlink");

    let svc = build_service(Some(base));
    let err = svc.parse_local(&symlink_path).await.unwrap_err();
    assert!(
        matches!(err, DomainError::PathTraversalBlocked { .. }),
        "Symlink escaping base dir should be blocked, got: {err:?}"
    );
}

// -----------------------------------------------------------------------
// 4. No base-dir configured — unrestricted (except `..` rejection)
// -----------------------------------------------------------------------

#[tokio::test]
async fn allows_absolute_path_when_no_base_dir() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let file = create_temp_file(tmp.path(), "test.txt", "No restriction");

    let svc = build_service(None);
    let doc = svc.parse_local(&file).await.expect("should parse OK");
    assert!(!doc.blocks.is_empty());
}

// -----------------------------------------------------------------------
// 5. Edge cases
// -----------------------------------------------------------------------

#[tokio::test]
async fn file_not_found_still_works() {
    let svc = build_service(None);
    let path = PathBuf::from("/nonexistent/path/to/file.txt");

    let err = svc.parse_local(&path).await.unwrap_err();
    assert!(
        matches!(err, DomainError::FileNotFound { .. }),
        "Expected FileNotFound, got: {err:?}"
    );
}

#[tokio::test]
async fn dotdot_error_message_contains_path() {
    let svc = build_service(None);
    let path = PathBuf::from("/safe/../etc/passwd");

    let err = svc.parse_local(&path).await.unwrap_err();
    match err {
        DomainError::PathTraversalBlocked { message } => {
            assert!(
                message.contains(".."),
                "Error message should mention '..': {message}"
            );
        }
        other => panic!("Expected PathTraversalBlocked, got: {other:?}"),
    }
}

#[tokio::test]
async fn base_dir_error_message_hides_canonical_path() {
    let base_tmp = tempfile::tempdir().expect("failed to create base dir");
    let other_tmp = tempfile::tempdir().expect("failed to create other dir");

    let base = base_tmp.path().canonicalize().unwrap();
    let outside = create_temp_file(other_tmp.path(), "leak.txt", "data");

    let svc = build_service(Some(base.clone()));
    let err = svc.parse_local(&outside).await.unwrap_err();
    match err {
        DomainError::PathTraversalBlocked { message } => {
            // The error message should NOT leak the base dir path to the caller
            assert!(
                !message.contains(&base.display().to_string()),
                "Error message should not reveal base dir: {message}"
            );
        }
        other => panic!("Expected PathTraversalBlocked, got: {other:?}"),
    }
}
