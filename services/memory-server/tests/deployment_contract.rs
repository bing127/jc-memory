//! Docker 与 Kubernetes 配置的运行路径契约测试。

use std::{fs, path::PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("服务目录的仓库父目录")
        .parent()
        .expect("仓库根目录")
        .to_path_buf()
}

#[test]
fn docker_image_contains_all_versioned_runtime_policies() {
    let root = repository_root();
    let dockerfile = fs::read_to_string(root.join("deploy/docker/Dockerfile")).unwrap();
    assert!(dockerfile.contains(
        "COPY --from=builder /src/config/retrieval/aliases.zh-CN.yaml /etc/jc-memory/retrieval/aliases.zh-CN.yaml"
    ));
    assert!(dockerfile.contains(
        "COPY --from=builder /src/config/security/quarantine.zh-CN.yaml /etc/jc-memory/security/quarantine.zh-CN.yaml"
    ));
    let server_yaml =
        fs::read_to_string(root.join("deploy/docker/config/server.yaml.example")).unwrap();
    assert!(server_yaml.contains("aliases_file: /etc/jc-memory/retrieval/aliases.zh-CN.yaml"));
    assert!(server_yaml.contains("rules_file: /etc/jc-memory/security/quarantine.zh-CN.yaml"));
}

#[test]
fn kubernetes_namespace_and_api_key_are_injected_from_secret() {
    let root = repository_root();
    let configmap = fs::read_to_string(root.join("deploy/kubernetes/configmap.yaml")).unwrap();
    assert!(!configmap.contains("namespace_id:"));
    let deployment = fs::read_to_string(root.join("deploy/kubernetes/deployment.yaml")).unwrap();
    assert!(deployment.contains("name: JC_MEMORY_NAMESPACE_ID"));
    assert!(deployment.contains("key: JC_MEMORY_NAMESPACE_ID"));
    assert!(deployment.contains("name: JC_MEMORY_API_KEY"));
    assert!(deployment.contains("key: JC_MEMORY_API_KEY"));
    let secret = fs::read_to_string(root.join("deploy/kubernetes/secret.example.yaml")).unwrap();
    assert!(secret.contains("JC_MEMORY_NAMESPACE_ID:"));
}
