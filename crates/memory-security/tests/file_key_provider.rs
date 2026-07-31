//! 文件型密钥提供者的持久化与安全边界契约。

use std::fmt::Write as _;

use jc_memory_security::{FileKeyProvider, KeyError, KeyProvider};
use tempfile::tempdir;

#[test]
fn file_provider_persists_key_and_rotation_metadata() {
    let directory = tempdir().expect("创建临时目录");
    let root = directory.path().join("keys");
    let provider = FileKeyProvider::open(&root).expect("打开文件 Provider");

    let first = provider
        .insert("primary", [7; 32], None)
        .expect("写入第一把密钥");
    let second = provider
        .insert("rotated", [9; 32], Some("primary"))
        .expect("写入轮换密钥");

    assert_eq!(first.version, 1);
    assert_eq!(second.version, 2);
    assert_eq!(second.rotated_from.as_deref(), Some("primary"));
    assert_eq!(*provider.get_key("rotated").expect("读取密钥"), [9; 32]);
    assert_eq!(provider.metadata("primary").expect("读取元数据"), first);

    let reopened = FileKeyProvider::open(&root).expect("重新打开文件 Provider");
    assert_eq!(
        *reopened.get_key("primary").expect("读取持久化密钥"),
        [7; 32]
    );
    assert_eq!(
        reopened.metadata("rotated").expect("读取持久化元数据"),
        second
    );
    let third = reopened
        .insert("third", [11; 32], Some("rotated"))
        .expect("重启后继续轮换");
    assert_eq!(third.version, 3);
}

#[test]
fn file_provider_generate_uses_a_nonzero_random_key() {
    let directory = tempdir().expect("创建临时目录");
    let provider = FileKeyProvider::open(directory.path().join("keys")).expect("打开文件 Provider");
    provider.generate("generated", None).expect("生成随机密钥");

    let key = provider.get_key("generated").expect("读取随机密钥");
    assert_ne!(*key, [0; 32]);
}

#[test]
fn file_provider_rejects_unsafe_ids_and_duplicate_without_overwrite() {
    let directory = tempdir().expect("创建临时目录");
    let provider = FileKeyProvider::open(directory.path().join("keys")).expect("打开文件 Provider");

    for id in ["", ".", "..", "../escape", "a/b", "a\\b", "bad id", "a\n"] {
        assert_eq!(
            provider.insert(id, [1; 32], None),
            Err(KeyError::InvalidKeyId)
        );
    }

    provider
        .insert("stable", [3; 32], None)
        .expect("写入稳定密钥");
    assert_eq!(
        provider.insert("stable", [4; 32], None),
        Err(KeyError::InvalidKeyId)
    );
    assert_eq!(*provider.get_key("stable").expect("读取原密钥"), [3; 32]);
}

#[test]
fn file_provider_rejects_corrupt_record_without_leaking_material() {
    let directory = tempdir().expect("创建临时目录");
    let root = directory.path().join("keys");
    let provider = FileKeyProvider::open(&root).expect("打开文件 Provider");
    provider
        .insert("primary", [0xabu8; 32], None)
        .expect("写入密钥");

    std::fs::write(root.join("primary.jck"), b"truncated").expect("破坏密钥记录");
    let error = provider.get_key("primary").expect_err("损坏记录必须失败");
    assert_eq!(error, KeyError::CorruptRecord);
    let mut rendered = String::new();
    write!(&mut rendered, "{error:?}").expect("格式化错误");
    assert!(!rendered.contains("ab"));
}

#[test]
fn file_provider_rejects_record_when_filename_and_embedded_id_differ() {
    let directory = tempdir().expect("创建临时目录");
    let root = directory.path().join("keys");
    let provider = FileKeyProvider::open(&root).expect("打开文件 Provider");
    provider
        .insert("primary", [0xabu8; 32], None)
        .expect("写入密钥");

    std::fs::copy(root.join("primary.jck"), root.join("other.jck")).expect("复制密钥记录");
    assert_eq!(
        provider.get_key("other").expect_err("文件名不匹配必须失败"),
        KeyError::CorruptRecord
    );
}

#[cfg(unix)]
#[test]
fn file_provider_rejects_symlink_records() {
    let directory = tempdir().expect("创建临时目录");
    let root = directory.path().join("keys");
    let provider = FileKeyProvider::open(&root).expect("打开文件 Provider");
    provider
        .insert("primary", [0xabu8; 32], None)
        .expect("写入密钥");

    std::os::unix::fs::symlink(root.join("primary.jck"), root.join("linked.jck"))
        .expect("创建符号链接");
    assert_eq!(
        provider.get_key("linked").expect_err("符号链接必须失败"),
        KeyError::CorruptRecord
    );
}

#[cfg(unix)]
#[test]
fn file_provider_rejects_symlink_lock() {
    let directory = tempdir().expect("创建临时目录");
    let root = directory.path().join("keys");
    std::fs::create_dir(&root).expect("创建密钥目录");
    std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .expect("收紧密钥目录权限");
    std::os::unix::fs::symlink(directory.path().join("outside.lock"), root.join(".lock"))
        .expect("创建锁符号链接");

    assert_eq!(
        FileKeyProvider::open(&root).expect_err("锁符号链接必须失败"),
        KeyError::InvalidStore
    );
}

#[cfg(unix)]
#[test]
fn file_provider_uses_restricted_unix_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempdir().expect("创建临时目录");
    let root = directory.path().join("keys");
    let provider = FileKeyProvider::open(&root).expect("打开文件 Provider");
    provider.insert("primary", [5; 32], None).expect("写入密钥");

    let root_mode = std::fs::metadata(&root)
        .expect("读取目录权限")
        .permissions()
        .mode()
        & 0o777;
    let file_mode = std::fs::metadata(root.join("primary.jck"))
        .expect("读取文件权限")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(root_mode, 0o700);
    assert_eq!(file_mode, 0o600);
}

#[cfg(unix)]
#[test]
fn file_provider_rejects_key_file_with_broadened_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempdir().expect("创建临时目录");
    let root = directory.path().join("keys");
    let provider = FileKeyProvider::open(&root).expect("打开文件 Provider");
    provider.insert("primary", [6; 32], None).expect("写入密钥");
    std::fs::set_permissions(
        root.join("primary.jck"),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("放宽密钥文件权限");

    assert_eq!(
        provider.get_key("primary").expect_err("权限过宽必须失败"),
        KeyError::PermissionDenied
    );
}
