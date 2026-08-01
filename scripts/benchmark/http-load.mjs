#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { hostname, platform, release } from "node:os";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULT_TIMEOUT_MS = 30_000;
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "::1", "[::1]"]);

export function nearestRank(values, percentile) {
  if (!values.length) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const rank = Math.min(
    sorted.length,
    Math.max(1, Math.ceil(percentile * sorted.length)),
  );
  return sorted[rank - 1];
}

export function assertLoadTarget(url, allowExternal) {
  if (!/^https?:$/.test(url.protocol)) {
    throw new Error("压测目标必须使用 HTTP 或 HTTPS");
  }
  if (LOOPBACK_HOSTS.has(url.hostname)) return;
  if (url.protocol !== "https:" || !allowExternal) {
    throw new Error("默认只允许压测 loopback；外部 HTTPS 目标必须显式使用 --allow-external");
  }
}

export function buildScenarioReport({
  name,
  requests,
  concurrency,
  elapsedMs,
  latencies,
  statusCounts,
  errors,
  realRustSqlite,
}) {
  const success = Object.entries(statusCounts)
    .filter(([status]) => Number(status) >= 200 && Number(status) < 300)
    .reduce((total, [, count]) => total + count, 0);
  const failure = requests - success;
  const max = latencies.length ? Math.max(...latencies) : null;
  return {
    name,
    requests,
    concurrency,
    success,
    failure,
    status_counts: Object.fromEntries(
      Object.entries(statusCounts).sort(([left], [right]) => left.localeCompare(right)),
    ),
    errors: Object.fromEntries(Object.entries(errors).sort(([left], [right]) => left.localeCompare(right))),
    throughput_rps: elapsedMs > 0 ? requests / (elapsedMs / 1_000) : null,
    latency_ms: {
      p50: nearestRank(latencies, 0.5),
      p95: nearestRank(latencies, 0.95),
      p99: nearestRank(latencies, 0.99),
      max,
    },
    real_rust_sqlite: realRustSqlite,
  };
}

function requestBody(template, index) {
  if (template === undefined) return undefined;
  if (typeof template !== "object" || template === null) return String(template);
  const body = structuredClone(template);
  if (typeof body.request_id === "string") {
    body.request_id = `${body.request_id}-${index + 1}`;
  }
  return JSON.stringify(body);
}

async function executeRequest(url, scenario, index, options) {
  const started = performance.now();
  const headers = { accept: "application/json" };
  const body = requestBody(scenario.body, index);
  if (body !== undefined) {
    headers["content-type"] = "application/json";
  }
  if (options.apiKey) headers["x-api-key"] = options.apiKey;
  if (options.sessionCookie) headers.cookie = options.sessionCookie;
  if (options.origin) headers.origin = options.origin;
  try {
    const response = await fetch(url, {
      method: scenario.method,
      headers,
      body,
      signal: AbortSignal.timeout(options.timeoutMs),
      redirect: "error",
    });
    await response.arrayBuffer();
    return {
      status: response.status,
      latency: Math.max(0, performance.now() - started),
    };
  } catch (error) {
    return {
      error: error instanceof DOMException && error.name === "TimeoutError"
        ? "timeout"
        : "transport_error",
      latency: Math.max(0, performance.now() - started),
    };
  }
}

export async function runScenario(baseUrl, scenario, options = {}) {
  const requests = scenario.requests;
  const concurrency = Math.min(scenario.concurrency, requests);
  const apiKey = options.apiKey;
  const timeoutMs = scenario.timeout_ms ?? options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const url = new URL(scenario.path, baseUrl);
  let next = 0;
  const latencies = [];
  const statusCounts = {};
  const errors = {};
  const started = performance.now();
  async function worker() {
    while (true) {
      const index = next;
      next += 1;
      if (index >= requests) return;
      const result = await executeRequest(url, scenario, index, {
        apiKey,
        sessionCookie: options.sessionCookie,
        origin: options.origin,
        timeoutMs,
      });
      latencies.push(result.latency);
      if (result.status !== undefined) {
        const status = String(result.status);
        statusCounts[status] = (statusCounts[status] ?? 0) + 1;
      } else {
        errors[result.error] = (errors[result.error] ?? 0) + 1;
      }
    }
  }
  await Promise.all(Array.from({ length: concurrency }, worker));
  return buildScenarioReport({
    name: scenario.name,
    requests,
    concurrency,
    elapsedMs: performance.now() - started,
    latencies,
    statusCounts,
    errors,
    realRustSqlite: options.realRustSqlite === true,
  });
}

async function warmupScenario(baseUrl, scenario, warmupRequests, options) {
  if (!warmupRequests) return;
  await runScenario(
    baseUrl,
    { ...scenario, requests: warmupRequests, name: `${scenario.name}-warmup` },
    options,
  );
}

function gitValue(args) {
  try {
    return execFileSync("git", args, { encoding: "utf8" }).trim();
  } catch {
    return "unknown";
  }
}

function provenance() {
  return {
    git_commit: gitValue(["rev-parse", "HEAD"]),
    dirty: gitValue(["status", "--porcelain"]) !== "",
    platform: `${platform()}-${release()}-${hostname()}`,
  };
}

export function parseArgs(argv) {
  const result = { allowExternal: false };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--allow-external") {
      result.allowExternal = true;
    } else if (argument === "--config" || argument === "--output") {
      const value = argv[index + 1];
      if (!value) throw new Error(`${argument} 需要参数`);
      result[argument.slice(2)] = value;
      index += 1;
    } else {
      throw new Error(`不支持的参数: ${argument}`);
    }
  }
  if (!result.config || !result.output) {
    throw new Error("必须提供 --config 和 --output");
  }
  return result;
}

export async function runFromConfig(configPath, outputPath, options = {}) {
  const config = JSON.parse(await readFile(configPath, "utf8"));
  const baseUrl = new URL(config.base_url);
  assertLoadTarget(baseUrl, options.allowExternal === true);
  const configDirectory = dirname(resolve(configPath));
  const apiKey = process.env.JC_MEMORY_API_KEY;
  const sessionCookie = process.env.JC_WEB_SESSION_COOKIE;
  const origin = process.env.JC_WEB_ORIGIN;
  const scenarios = [];
  for (const input of config.scenarios) {
    const body = input.body_file
      ? JSON.parse(await readFile(resolve(configDirectory, input.body_file), "utf8"))
      : input.body;
    const scenario = { ...input, body };
    await warmupScenario(baseUrl, scenario, config.warmup_requests ?? 0, {
      apiKey,
      sessionCookie,
      origin,
      timeoutMs: config.timeout_ms,
      realRustSqlite: config.real_rust_sqlite,
    });
    scenarios.push(await runScenario(baseUrl, scenario, {
      apiKey,
      sessionCookie,
      origin,
      timeoutMs: config.timeout_ms,
      realRustSqlite: config.real_rust_sqlite,
    }));
  }
  const report = {
    schema_version: "jcmem.load-test.v1",
    provenance: provenance(),
    config: {
      target: `${baseUrl.protocol}//${baseUrl.host}`,
      warmup_requests: config.warmup_requests ?? 0,
      timeout_ms: config.timeout_ms ?? DEFAULT_TIMEOUT_MS,
      real_rust_sqlite: config.real_rust_sqlite === true,
    },
    scenarios,
  };
  await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  return report;
}

export async function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  await runFromConfig(args.config, args.output, args);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
