//! 将版本化 JSON 报告转换为中文 Markdown。

use std::{env, fs, process};

use jc_memory_benchmark_harness::BenchmarkReport;

fn main() {
    let arguments: Vec<_> = env::args().collect();
    if arguments.len() != 3 {
        eprintln!("用法：{} <report.json> <report.md>", arguments[0]);
        process::exit(2);
    }
    let report = match fs::read_to_string(&arguments[1])
        .map_err(|error| error.to_string())
        .and_then(|text| {
            serde_json::from_str::<BenchmarkReport>(&text).map_err(|error| error.to_string())
        }) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("读取 Benchmark 报告失败：{error}");
            process::exit(1);
        }
    };
    if let Err(error) = fs::write(&arguments[2], report.to_markdown()) {
        eprintln!("写入 Markdown 报告失败：{error}");
        process::exit(1);
    }
}
