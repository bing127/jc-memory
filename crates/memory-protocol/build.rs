//! 从仓库内稳定 Proto 契约生成 Rust 类型。

use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let proto_root = manifest_dir.join("../../proto");
    let memory_proto = proto_root.join("jiangcheng/memory/v1/memory.proto");

    println!("cargo:rerun-if-changed={}", memory_proto.display());
    println!(
        "cargo:rerun-if-changed={}",
        proto_root.join("buf.yaml").display()
    );
    tonic_prost_build::configure().compile_protos(&[memory_proto], &[proto_root])?;
    Ok(())
}
