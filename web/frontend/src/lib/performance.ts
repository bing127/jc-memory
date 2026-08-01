import { z } from "zod";

import type { PerformanceOperationName, PerformanceSnapshot } from "./contracts.js";

const PERFORMANCE_HISTORY_LIMIT = 60;

const nullableMetric = z.number().finite().nonnegative().nullable();

const scenarioSchema = z.object({
  name: z.string().min(1),
  requests: z.number().int().nonnegative(),
  concurrency: z.number().int().positive(),
  success: z.number().int().nonnegative(),
  failure: z.number().int().nonnegative(),
  status_counts: z.record(z.string(), z.number().int().nonnegative()),
  errors: z.record(z.string(), z.number().int().nonnegative()),
  throughput_rps: nullableMetric,
  latency_ms: z.object({
    p50: nullableMetric,
    p95: nullableMetric,
    p99: nullableMetric,
    max: nullableMetric,
  }).strict(),
  real_rust_sqlite: z.boolean(),
}).strict();

const reportSchema = z.object({
  schema_version: z.literal("jcmem.load-test.v1"),
  provenance: z.object({
    git_commit: z.string().min(1),
    dirty: z.boolean(),
    platform: z.string().min(1),
  }).strict(),
  config: z.object({
    target: z.string().min(1),
    warmup_requests: z.number().int().nonnegative(),
    timeout_ms: z.number().int().positive(),
    real_rust_sqlite: z.boolean(),
  }).strict(),
  scenarios: z.array(scenarioSchema).min(1),
}).strict();

export type LoadTestScenario = z.infer<typeof scenarioSchema>;
export type LoadTestReport = z.infer<typeof reportSchema>;

export interface LoadTestSummary {
  requests: number;
  success: number;
  failure: number;
  successRate: number | null;
  throughputRps: number | null;
  status429: number;
  latencyMax: {
    p50: number | null;
    p95: number | null;
    p99: number | null;
  };
}

export interface PerformanceHistoryEntry {
  sampledAt: string;
  snapshot: PerformanceSnapshot | null;
}

export interface OperationSeriesPoint {
  sampledAt: string;
  label: string;
  gap: boolean;
  total: number | null;
  rps: number | null;
  successRate: number | null;
  inFlight: number | null;
  p50: number | null;
  p95: number | null;
  p99: number | null;
  rateLimited: number | null;
  timeouts: number | null;
}

export function parseLoadTestReport(value: unknown): LoadTestReport {
  const result = reportSchema.safeParse(value);
  if (!result.success) {
    const version = typeof value === "object" && value !== null && "schema_version" in value
      ? String(value.schema_version)
      : "未知";
    if (version !== "jcmem.load-test.v1") {
      throw new Error(`不支持的压测报告版本：${version}`);
    }
    throw new Error("压测报告格式不完整或包含无效指标");
  }
  return result.data;
}

export function summarizeLoadTestReport(report: LoadTestReport): LoadTestSummary {
  const requests = report.scenarios.reduce((total, scenario) => total + scenario.requests, 0);
  const success = report.scenarios.reduce((total, scenario) => total + scenario.success, 0);
  const failure = report.scenarios.reduce((total, scenario) => total + scenario.failure, 0);
  const throughputValues = report.scenarios
    .map((scenario) => scenario.throughput_rps)
    .filter((value): value is number => value !== null);
  return {
    requests,
    success,
    failure,
    successRate: requests > 0 ? (success / requests) * 100 : null,
    throughputRps: throughputValues.length
      ? throughputValues.reduce((total, value) => total + value, 0) / throughputValues.length
      : null,
    status429: report.scenarios.reduce(
      (total, scenario) => total + (scenario.status_counts["429"] ?? 0),
      0,
    ),
    latencyMax: {
      p50: maximumCollected(report.scenarios.map((scenario) => scenario.latency_ms.p50)),
      p95: maximumCollected(report.scenarios.map((scenario) => scenario.latency_ms.p95)),
      p99: maximumCollected(report.scenarios.map((scenario) => scenario.latency_ms.p99)),
    },
  };
}

function maximumCollected(values: Array<number | null>): number | null {
  const collected = values.filter((value): value is number => value !== null);
  return collected.length ? Math.max(...collected) : null;
}

export function formatMetric(value: number | null, unit: string): string {
  return value === null ? "未采集" : `${value.toLocaleString("zh-CN", { maximumFractionDigits: 2 })} ${unit}`;
}

export function reportChartData(report: LoadTestReport) {
  return {
    latency: report.scenarios.map((scenario) => ({
      name: scenario.name,
      p50: scenario.latency_ms.p50,
      p95: scenario.latency_ms.p95,
      p99: scenario.latency_ms.p99,
    })),
    throughput: report.scenarios.map((scenario) => ({
      name: scenario.name,
      concurrency: scenario.concurrency,
      throughput: scenario.throughput_rps,
    })),
    statuses: Object.entries(
      report.scenarios.reduce<Record<string, number>>((counts, scenario) => {
        for (const [status, count] of Object.entries(scenario.status_counts)) {
          counts[status] = (counts[status] ?? 0) + count;
        }
        return counts;
      }, {}),
    )
      .sort(([left], [right]) => Number(left) - Number(right))
      .map(([status, count]) => ({ status, count })),
  };
}

export function appendPerformanceHistory(
  history: PerformanceHistoryEntry[],
  snapshot: PerformanceSnapshot | null,
  sampledAt = snapshot?.sampledAt ?? new Date().toISOString(),
): PerformanceHistoryEntry[] {
  return [...history, { sampledAt, snapshot }].slice(-PERFORMANCE_HISTORY_LIMIT);
}

export function deriveOperationSeries(
  history: PerformanceHistoryEntry[],
  operationName: PerformanceOperationName,
): OperationSeriesPoint[] {
  let previous: { total: number; sampledAtMs: number } | null = null;
  return history.map((entry) => {
    if (!entry.snapshot) return gapPoint(entry.sampledAt);
    const operation = entry.snapshot.operations.find((item) => item.operation === operationName);
    const total = operation?.total ?? 0;
    const sampledAtMs = Date.parse(entry.sampledAt);
    const elapsedSeconds = previous ? (sampledAtMs - previous.sampledAtMs) / 1_000 : 0;
    const rps = previous && elapsedSeconds > 0 && total >= previous.total
      ? (total - previous.total) / elapsedSeconds
      : null;
    previous = { total, sampledAtMs };
    return {
      sampledAt: entry.sampledAt,
      label: sampleTimeLabel(entry.sampledAt),
      gap: false,
      total,
      rps,
      successRate: total > 0 ? ((operation?.success ?? 0) / total) * 100 : null,
      inFlight: entry.snapshot.inFlight,
      p50: operation?.latencyMs.p50 ?? null,
      p95: operation?.latencyMs.p95 ?? null,
      p99: operation?.latencyMs.p99 ?? null,
      rateLimited: operation?.rateLimited ?? 0,
      timeouts: operation?.timeouts ?? 0,
    };
  });
}

function gapPoint(sampledAt: string): OperationSeriesPoint {
  return {
    sampledAt,
    label: sampleTimeLabel(sampledAt),
    gap: true,
    total: null,
    rps: null,
    successRate: null,
    inFlight: null,
    p50: null,
    p95: null,
    p99: null,
    rateLimited: null,
    timeouts: null,
  };
}

function sampleTimeLabel(sampledAt: string): string {
  const timestamp = Date.parse(sampledAt);
  if (!Number.isFinite(timestamp)) return sampledAt;
  return new Date(timestamp).toLocaleTimeString("zh-CN", { hour12: false });
}
