#![allow(missing_docs)]

use std::{fs, time::Duration};

use jc_memory_million_fragments::{
    Checkpoint, CheckpointError, Generator, ReportError, StressReport, config_digest,
    generate_smoke, load_checkpoint, resume_sequence, resume_smoke, save_checkpoint,
    throughput_per_second,
};

#[test]
fn generator_is_streaming_and_checkpoint_round_trip_resumes_next_sequence() {
    let mut generator = Generator::new(7, 1_000);
    assert_eq!(generator.size_hint().0, 1_000);
    let first = generator.next().expect("首个 Fragment");
    assert_eq!(first.sequence, 0);
    let checkpoint = Checkpoint::new(config_digest("seed=7;fragments=1000"), 511, 512);
    let directory = tempfile::tempdir().expect("临时目录");
    let path = directory.path().join("checkpoint.json");
    save_checkpoint(&path, &checkpoint).expect("保存 Checkpoint");
    let loaded = load_checkpoint(&path).expect("加载 Checkpoint");
    assert_eq!(
        resume_sequence(&loaded, &checkpoint.config_digest).expect("恢复 sequence"),
        512
    );
    assert_eq!(loaded, checkpoint);
}

#[test]
fn crash_recovery_keeps_last_committed_batch_and_rejects_config_mismatch() {
    let directory = tempfile::tempdir().expect("临时目录");
    let path = directory.path().join("checkpoint.json");
    let outcome = generate_smoke(1_000, 128, &path, Some(3)).expect("模拟批次崩溃");
    assert!(outcome.crashed);
    assert_eq!(outcome.checkpoint.last_sequence, 383);
    let error = resume_sequence(&outcome.checkpoint, &config_digest("different"))
        .expect_err("篡改 config digest 必须拒绝");
    assert!(matches!(error, CheckpointError::ConfigMismatch { .. }));
    assert!(path.exists());
    let resumed = resume_smoke(1_000, 128, &path).expect("从 Checkpoint 继续");
    assert!(!resumed.crashed);
    assert_eq!(resumed.checkpoint.last_sequence, 999);
}

#[test]
fn report_validation_rejects_missing_measurement_phase_and_bad_raw_count() {
    let mut report = StressReport::smoke(1_000, config_digest("seed=7;fragments=1000"));
    assert!(report.validate().is_ok());
    report.measurement_complete = false;
    assert!(matches!(
        report.validate(),
        Err(ReportError::MissingMeasurement)
    ));
    report.measurement_complete = true;
    report.generated_fragments = 999;
    assert!(matches!(
        report.validate(),
        Err(ReportError::RawCountMismatch { .. })
    ));
}

#[test]
fn checkpoint_write_is_atomic_from_the_callers_perspective() {
    let directory = tempfile::tempdir().expect("临时目录");
    let path = directory.path().join("checkpoint.json");
    let checkpoint = Checkpoint::new(config_digest("stable"), 3, 4);
    save_checkpoint(&path, &checkpoint).expect("保存 Checkpoint");
    let entries: Vec<_> = fs::read_dir(directory.path()).expect("读取目录").collect();
    assert_eq!(entries.len(), 1);
}

#[test]
fn measured_write_throughput_uses_committed_count_and_elapsed_time() {
    assert_eq!(
        throughput_per_second(1_000, Duration::from_secs(2)),
        Some(500.0)
    );
    assert_eq!(throughput_per_second(0, Duration::from_secs(2)), None);
    assert_eq!(throughput_per_second(1_000, Duration::ZERO), None);
}
