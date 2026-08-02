use std::{
    fmt::Write as _,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use jc_memory_protocol::MemoryCommand;

const OPERATION_COUNT: usize = 8;
const TRANSPORT_COUNT: usize = 2;
const BUCKETS_MS: [u64; 10] = [5, 10, 25, 50, 100, 250, 500, 1_000, 5_000, 30_000];

#[derive(Clone, Copy)]
pub(crate) enum MetricOperation {
    Observe,
    Commit,
    Recall,
    Stats,
    Correct,
    Forget,
    Explain,
    Command,
}

impl MetricOperation {
    pub(crate) fn from_command(command: &MemoryCommand) -> Self {
        match command {
            MemoryCommand::Observe(_) => Self::Observe,
            MemoryCommand::Commit(_) => Self::Commit,
            MemoryCommand::Recall(_) => Self::Recall,
            MemoryCommand::Stats(_) => Self::Stats,
            MemoryCommand::Correct(_) => Self::Correct,
            MemoryCommand::Forget(_) => Self::Forget,
            MemoryCommand::Explain(_) => Self::Explain,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Observe => 0,
            Self::Commit => 1,
            Self::Recall => 2,
            Self::Stats => 3,
            Self::Correct => 4,
            Self::Forget => 5,
            Self::Explain => 6,
            Self::Command => 7,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Commit => "commit",
            Self::Recall => "recall",
            Self::Stats => "stats",
            Self::Correct => "correct",
            Self::Forget => "forget",
            Self::Explain => "explain",
            Self::Command => "command",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum MetricTransport {
    Http,
    Grpc,
}

impl MetricTransport {
    const fn index(self) -> usize {
        match self {
            Self::Http => 0,
            Self::Grpc => 1,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Grpc => "grpc",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum MetricOutcome {
    Success,
    Error,
    RateLimited,
    Timeout,
}

impl MetricOutcome {
    pub(crate) fn from_error(error: Option<&crate::ServerError>) -> Self {
        match error {
            None => Self::Success,
            Some(crate::ServerError::RateLimited) => Self::RateLimited,
            Some(crate::ServerError::Timeout { .. }) => Self::Timeout,
            Some(_) => Self::Error,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
        }
    }
}

struct MetricEntry {
    requests_total: AtomicU64,
    outcomes: [AtomicU64; 4],
    duration_buckets: [AtomicU64; BUCKETS_MS.len() + 1],
    duration_sum_micros: AtomicU64,
}

impl MetricEntry {
    fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            outcomes: std::array::from_fn(|_| AtomicU64::new(0)),
            duration_buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            duration_sum_micros: AtomicU64::new(0),
        }
    }

    fn record(&self, outcome: MetricOutcome, elapsed: Duration) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        self.outcomes[outcome.index()].fetch_add(1, Ordering::Relaxed);
        let elapsed_micros = elapsed.as_micros();
        for (index, boundary) in BUCKETS_MS.iter().enumerate() {
            if elapsed_micros <= u128::from(*boundary) * 1_000 {
                self.duration_buckets[index].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.duration_buckets[BUCKETS_MS.len()].fetch_add(1, Ordering::Relaxed);
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.duration_sum_micros
            .fetch_add(micros, Ordering::Relaxed);
    }

    fn render(&self, output: &mut String, operation: MetricOperation, transport: MetricTransport) {
        let labels = format!(
            "operation=\"{}\",transport=\"{}\"",
            operation.label(),
            transport.label()
        );
        let total = self.requests_total.load(Ordering::Relaxed);
        let _ = writeln!(output, "jc_memory_requests_total{{{labels}}} {total}");
        for outcome in [
            MetricOutcome::Success,
            MetricOutcome::Error,
            MetricOutcome::RateLimited,
            MetricOutcome::Timeout,
        ] {
            let count = self.outcomes[outcome.index()].load(Ordering::Relaxed);
            let _ = writeln!(
                output,
                "jc_memory_requests_total{{{labels},outcome=\"{}\"}} {count}",
                outcome.label()
            );
        }
        for (index, boundary) in BUCKETS_MS.iter().enumerate() {
            let count = self.duration_buckets[index].load(Ordering::Relaxed);
            let _ = writeln!(
                output,
                "jc_memory_request_duration_seconds_bucket{{{labels},le=\"{}\"}} {}",
                seconds_label(*boundary),
                count
            );
        }
        let count = self.duration_buckets[BUCKETS_MS.len()].load(Ordering::Relaxed);
        let sum_micros = self.duration_sum_micros.load(Ordering::Relaxed);
        let _ = writeln!(
            output,
            "jc_memory_request_duration_seconds_bucket{{{labels},le=\"+Inf\"}} {count}"
        );
        let _ = writeln!(
            output,
            "jc_memory_request_duration_seconds_count{{{labels}}} {count}"
        );
        let seconds = sum_micros / 1_000_000;
        let micros = sum_micros % 1_000_000;
        let _ = writeln!(
            output,
            "jc_memory_request_duration_seconds_sum{{{labels}}} {seconds}.{micros:06}"
        );
    }
}

fn seconds_label(milliseconds: u64) -> String {
    if milliseconds.is_multiple_of(1_000) {
        (milliseconds / 1_000).to_string()
    } else {
        format!("0.{milliseconds:03}")
    }
}

pub(crate) struct ServerMetrics {
    entries: [MetricEntry; OPERATION_COUNT * TRANSPORT_COUNT],
}

impl ServerMetrics {
    pub(crate) fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| MetricEntry::new()),
        }
    }

    pub(crate) fn record(
        &self,
        operation: MetricOperation,
        transport: MetricTransport,
        outcome: MetricOutcome,
        elapsed: Duration,
    ) {
        self.entry(operation, transport).record(outcome, elapsed);
    }

    pub(crate) fn render(&self, in_flight: usize) -> String {
        let mut output = String::new();
        output.push_str(
            "# HELP jc_memory_requests_total Memory requests by operation and outcome.\n",
        );
        output.push_str("# TYPE jc_memory_requests_total counter\n");
        output.push_str("# HELP jc_memory_request_duration_seconds Request duration histogram.\n");
        output.push_str("# TYPE jc_memory_request_duration_seconds histogram\n");
        output.push_str("# HELP jc_memory_requests_in_flight Current in-flight requests.\n");
        output.push_str("# TYPE jc_memory_requests_in_flight gauge\n");
        for operation in [
            MetricOperation::Observe,
            MetricOperation::Commit,
            MetricOperation::Recall,
            MetricOperation::Stats,
            MetricOperation::Correct,
            MetricOperation::Forget,
            MetricOperation::Explain,
            MetricOperation::Command,
        ] {
            for transport in [MetricTransport::Http, MetricTransport::Grpc] {
                self.entry(operation, transport)
                    .render(&mut output, operation, transport);
            }
        }
        let _ = writeln!(output, "jc_memory_requests_in_flight {in_flight}");
        output.push_str("jc_memory_server_ready 1\n");
        output
    }

    fn entry(&self, operation: MetricOperation, transport: MetricTransport) -> &MetricEntry {
        &self.entries[operation.index() * TRANSPORT_COUNT + transport.index()]
    }
}

impl MetricOutcome {
    const fn index(self) -> usize {
        match self {
            Self::Success => 0,
            Self::Error => 1,
            Self::RateLimited => 2,
            Self::Timeout => 3,
        }
    }
}
