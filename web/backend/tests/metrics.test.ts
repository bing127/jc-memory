import { describe, expect, test } from "vitest";

import { parseServerMetrics } from "../src/metrics.js";

const fixture = `
jc_memory_requests_total{operation="observe",transport="http"} 3
jc_memory_requests_total{operation="observe",transport="http",outcome="success"} 2
jc_memory_requests_total{operation="observe",transport="http",outcome="error"} 1
jc_memory_requests_total{operation="observe",transport="http",outcome="rate_limited"} 0
jc_memory_requests_total{operation="observe",transport="http",outcome="timeout"} 0
jc_memory_requests_total{operation="observe",transport="grpc"} 1
jc_memory_requests_total{operation="observe",transport="grpc",outcome="success"} 1
jc_memory_request_duration_seconds_bucket{operation="observe",transport="http",le="0.005"} 1
jc_memory_request_duration_seconds_bucket{operation="observe",transport="http",le="0.010"} 2
jc_memory_request_duration_seconds_bucket{operation="observe",transport="http",le="0.025"} 3
jc_memory_request_duration_seconds_bucket{operation="observe",transport="http",le="+Inf"} 3
jc_memory_request_duration_seconds_bucket{operation="observe",transport="grpc",le="0.005"} 0
jc_memory_request_duration_seconds_bucket{operation="observe",transport="grpc",le="0.010"} 0
jc_memory_request_duration_seconds_bucket{operation="observe",transport="grpc",le="0.025"} 1
jc_memory_request_duration_seconds_bucket{operation="observe",transport="grpc",le="+Inf"} 1
jc_memory_requests_in_flight 3
jc_memory_server_ready 1
`;

describe("memory-server metrics parser", () => {
  test("aggregates fixed transports and derives bounded latency percentiles", () => {
    const snapshot = parseServerMetrics(fixture, "2026-08-01T10:00:00.000Z");

    expect(snapshot.inFlight).toBe(3);
    expect(snapshot.sampledAt).toBe("2026-08-01T10:00:00.000Z");
    expect(snapshot.operations).toEqual([
      {
        operation: "observe",
        total: 4,
        success: 3,
        error: 1,
        rateLimited: 0,
        timeouts: 0,
        latencyMs: { p50: 10, p95: 25, p99: 25 },
      },
    ]);
  });

  test("rejects non-finite values and high-cardinality labels", () => {
    expect(() => parseServerMetrics("jc_memory_requests_in_flight NaN"))
      .toThrow(/有限/);
    expect(() => parseServerMetrics(
      'jc_memory_requests_total{operation="recall",transport="http",tenant="secret"} 1',
    )).toThrow(/label/i);
  });
});
