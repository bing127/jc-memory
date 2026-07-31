//! Million Fragments CLI；百万长测必须显式指定 fragments 和 output。

use std::{env, fs, path::PathBuf, process, time::Instant};

use jc_memory_million_fragments::{
    StressReport, config_digest, generate_smoke, throughput_per_second,
};

fn main() {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if let Err(error) = run(&arguments) {
        eprintln!("Million Fragments 运行失败：{error}");
        process::exit(1);
    }
}

fn run(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let fragments = option_value(arguments, "--fragments")
        .ok_or("必须显式传入 --fragments")?
        .parse::<u64>()?;
    let output = option_value(arguments, "--output").map(PathBuf::from);
    if fragments >= 1_000_000 && output.is_none() {
        return Err("百万长测必须显式传入 --output".into());
    }
    let checkpoint = output.as_ref().map_or_else(
        || PathBuf::from("million-fragments.checkpoint.json"),
        |path| path.with_extension("checkpoint.json"),
    );
    let started = Instant::now();
    let outcome = generate_smoke(fragments, 1_000, &checkpoint, None)?;
    let throughput =
        throughput_per_second(outcome.checkpoint.committed_fragments, started.elapsed());
    let report = StressReport::smoke(
        fragments,
        config_digest(&format!("fragments={fragments};batch_size=1000")),
    )
    .with_write_throughput(throughput);
    report.validate()?;
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(output) = output {
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(output, json)?;
    } else {
        println!("{json}");
    }
    if outcome.crashed {
        return Err("未预期的崩溃状态".into());
    }
    Ok(())
}

fn option_value(arguments: &[String], option: &str) -> Option<String> {
    arguments
        .windows(2)
        .find(|window| window[0] == option)
        .map(|window| window[1].clone())
}
