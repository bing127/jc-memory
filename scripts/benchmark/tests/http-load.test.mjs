import assert from "node:assert/strict";
import { test } from "node:test";

import {
  assertLoadTarget,
  buildScenarioReport,
  nearestRank,
} from "../http-load.mjs";

test("nearestRank returns the upper-ranked latency sample", () => {
  assert.equal(nearestRank([1, 2, 9, 10], 0.5), 2);
  assert.equal(nearestRank([1, 2, 9, 10], 0.95), 10);
});

test("external load targets require explicit opt-in", () => {
  assertLoadTarget(new URL("http://127.0.0.1:8987"), false);
  assert.throws(
    () => assertLoadTarget(new URL("https://memory.example.com"), false),
    /loopback|external/i,
  );
  assertLoadTarget(new URL("https://memory.example.com"), true);
});

test("scenario report keeps status counts consistent and redacts credentials", () => {
  const report = buildScenarioReport({
    name: "fixture",
    requests: 3,
    concurrency: 2,
    elapsedMs: 100,
    latencies: [10, 20],
    statusCounts: { "200": 2, "429": 1 },
    errors: {},
    realRustSqlite: true,
  });

  assert.equal(report.success, 2);
  assert.equal(report.failure, 1);
  assert.equal(report.status_counts["429"], 1);
  assert.equal(report.latency_ms.p95, 20);
  assert.equal(JSON.stringify(report).includes("sk-jcmemory-secret"), false);
});
