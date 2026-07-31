//! Cross-Year Runner 的目录级入口。

use jc_memory_benchmark_harness::cross_year::{Dataset, Error, generate};

/// 运行固定 Smoke 配置。
pub fn run_smoke() -> Result<Dataset, Error> {
    generate(7, 32)
}
