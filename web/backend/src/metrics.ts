const OPERATION_NAMES = [
  "observe",
  "commit",
  "recall",
  "stats",
  "correct",
  "forget",
  "explain",
  "command",
] as const;

type OperationName = (typeof OPERATION_NAMES)[number];
type Labels = Record<string, string>;

export interface PerformanceOperation {
  operation: OperationName;
  total: number;
  success: number;
  error: number;
  rateLimited: number;
  timeouts: number;
  latencyMs: { p50: number | null; p95: number | null; p99: number | null };
}

export interface PerformanceSnapshot {
  sampledAt: string;
  inFlight: number;
  operations: PerformanceOperation[];
}

interface OperationAccumulator {
  total: number;
  outcomes: Record<"success" | "error" | "rate_limited" | "timeout", number>;
  buckets: Map<number, number>;
}

const REQUEST_LABELS = new Set(["operation", "transport", "outcome"]);
const HISTOGRAM_LABELS = new Set(["operation", "transport", "le"]);

export function parseServerMetrics(
  input: string,
  sampledAt = new Date().toISOString(),
): PerformanceSnapshot {
  const operations = new Map<OperationName, OperationAccumulator>();
  let inFlight = 0;
  for (const rawLine of input.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const match = /^(\w+)(?:\{([^}]*)\})?\s+([^\s]+)$/.exec(line);
    if (!match) throw new Error("指标行格式无效");
    const metric = match[1];
    const rawLabels = match[2] ?? "";
    const rawValue = match[3];
    if (!metric || !rawValue) throw new Error("指标行缺少名称或数值");
    const value = parseMetricNumber(rawValue);
    const labels = parseLabels(rawLabels);
    if (metric === "jc_memory_requests_in_flight") {
      ensureLabels(labels, new Set());
      inFlight = assertNonNegativeInteger(value, "in_flight");
      continue;
    }
    if (metric === "jc_memory_server_ready") {
      ensureLabels(labels, new Set());
      continue;
    }
    const operation = operationFromLabel(labels.operation);
    if (!operation) continue;
    const transport = labels.transport;
    if (transport !== "http" && transport !== "grpc") {
      throw new Error("指标 transport label 无效");
    }
    const accumulator = operations.get(operation) ?? createAccumulator();
    operations.set(operation, accumulator);
    if (metric === "jc_memory_requests_total") {
      ensureLabels(labels, REQUEST_LABELS);
      if (labels.outcome) {
        if (!(labels.outcome in accumulator.outcomes)) {
          throw new Error("指标 outcome label 无效");
        }
        accumulator.outcomes[labels.outcome as keyof typeof accumulator.outcomes] +=
          assertNonNegativeInteger(value, "outcome");
      } else {
        accumulator.total += assertNonNegativeInteger(value, "total");
      }
    } else if (metric === "jc_memory_request_duration_seconds_bucket") {
      ensureLabels(labels, HISTOGRAM_LABELS);
      const boundaryLabel = labels.le;
      if (!boundaryLabel) throw new Error("指标 histogram 缺少 le label");
      const boundary = boundaryLabel === "+Inf"
        ? Infinity
        : parseMetricNumber(boundaryLabel) * 1_000;
      accumulator.buckets.set(
        boundary,
        (accumulator.buckets.get(boundary) ?? 0) + assertNonNegativeInteger(value, "bucket"),
      );
    } else if (
      metric === "jc_memory_request_duration_seconds_count" ||
      metric === "jc_memory_request_duration_seconds_sum"
    ) {
      ensureLabels(labels, new Set(["operation", "transport"]));
    }
  }

  return {
    sampledAt,
    inFlight,
    operations: OPERATION_NAMES
      .filter((operation) => operations.has(operation))
      .map((operation) => {
        const accumulator = operations.get(operation);
        if (!accumulator) throw new Error("指标 operation 缺失");
        const bucketTotal = accumulator.buckets.get(Infinity) ?? 0;
        const total = Math.max(accumulator.total, bucketTotal, sumOutcomes(accumulator));
        return {
          operation,
          total,
          success: accumulator.outcomes.success,
          error: accumulator.outcomes.error,
          rateLimited: accumulator.outcomes.rate_limited,
          timeouts: accumulator.outcomes.timeout,
          latencyMs: {
            p50: percentile(accumulator.buckets, total, 0.5),
            p95: percentile(accumulator.buckets, total, 0.95),
            p99: percentile(accumulator.buckets, total, 0.99),
          },
        };
      }),
  };
}

function createAccumulator(): OperationAccumulator {
  return {
    total: 0,
    outcomes: { success: 0, error: 0, rate_limited: 0, timeout: 0 },
    buckets: new Map(),
  };
}

function sumOutcomes(accumulator: OperationAccumulator): number {
  return Object.values(accumulator.outcomes).reduce((sum, value) => sum + value, 0);
}

function operationFromLabel(value: string | undefined): OperationName | undefined {
  return OPERATION_NAMES.find((operation) => operation === value);
}

function parseLabels(rawLabels: string): Labels {
  if (!rawLabels) return {};
  const labels: Labels = {};
  for (const part of rawLabels.split(",")) {
    const match = /^([a-zA-Z_][a-zA-Z0-9_]*)="((?:[^"\\]|\\.)*)"$/.exec(part.trim());
    if (!match) throw new Error("指标 label 格式无效");
    const labelName = match[1];
    const labelValue = match[2];
    if (!labelName || labelValue === undefined) throw new Error("指标 label 缺少名称或值");
    labels[labelName] = labelValue.replace(/\\([\\"])/g, "$1");
  }
  return labels;
}

function ensureLabels(labels: Labels, allowed: Set<string>): void {
  for (const key of Object.keys(labels)) {
    if (!allowed.has(key)) throw new Error(`指标 label 不受支持: ${key}`);
  }
}

function parseMetricNumber(rawValue: string): number {
  const value = Number(rawValue);
  if (!Number.isFinite(value)) throw new Error("指标值必须是有限数值");
  return value;
}

function assertNonNegativeInteger(value: number, field: string): number {
  if (!Number.isInteger(value) || value < 0) {
    throw new Error(`指标 ${field} 必须是非负整数`);
  }
  return value;
}

function percentile(buckets: Map<number, number>, total: number, quantile: number): number | null {
  if (!total) return null;
  const target = Math.ceil(total * quantile);
  let cumulative = 0;
  for (const [boundary, count] of [...buckets.entries()].sort(([left], [right]) => left - right)) {
    cumulative = Math.max(cumulative, count);
    if (cumulative >= target) return Number.isFinite(boundary) ? boundary : null;
  }
  return null;
}
