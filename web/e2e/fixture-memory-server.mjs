import { createServer } from "node:http";

const namespaceId = "00000000-0000-0000-0000-000000000001";
const apiKey = "sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t";
const runtimeRunId = "00000000-0000-7000-8000-000000000701";
const memoryPort = Number(process.env.JC_WEB_E2E_MEMORY_PORT ?? 8787);
let fragmentCount = 0;
let eventCount = 0;
let queryCount = 0;
let metricsRequestCount = 0;
let metricsFailure = false;
const requestCounts = {
  health: 0,
  ready: 0,
  metrics: 0,
  stats: 0,
  runtimeList: 0,
  runtimeDetail: 0,
  runtimeStart: 0,
};
const storedFragments = [];
const stateChains = new Map();
let stateCount = 0;
let stateVersionCount = 0;

const server = createServer(async (request, response) => {
  if (request.method === "GET" && request.url === "/__fixture/requests") {
    return json(response, requestCounts);
  }
  if (request.method === "POST" && request.url === "/__fixture/metrics-mode") {
    const body = JSON.parse(await readBody(request));
    if (typeof body.failure !== "boolean") {
      return json(response, { code: "invalid_argument" }, 400);
    }
    metricsFailure = body.failure;
    return json(response, { failure: metricsFailure });
  }
  if (request.method === "GET" && request.url === "/healthz") {
    requestCounts.health += 1;
    return json(response, { status: "ok" });
  }
  if (request.method === "GET" && request.url === "/readyz") {
    requestCounts.ready += 1;
    return json(response, { status: "ready" });
  }
  if (request.method === "GET" && request.url === "/metrics") {
    requestCounts.metrics += 1;
    if (metricsFailure) {
      response.writeHead(503, { "content-type": "text/plain" });
      return response.end("fixture metrics unavailable");
    }
    response.writeHead(200, { "content-type": "text/plain; version=0.0.4" });
    metricsRequestCount += 1;
    return response.end(metricsText(metricsRequestCount));
  }
  if (request.method === "POST" && request.url === "/v1/runtime/runs") {
    requestCounts.runtimeStart += 1;
    if (request.headers["x-api-key"] !== apiKey) {
      return json(response, { code: "unauthorized" }, 401);
    }
    const body = JSON.parse(await readBody(request));
    if (typeof body.prompt !== "string" || !body.prompt.trim()) {
      return json(response, { code: "invalid_argument" }, 400);
    }
    return json(response, {
      outcome: "completed",
      run_id: runtimeRunId,
      state: "completed",
      response: { text: "Fixture FinalResponse" },
      approval_id: null,
    }, 201);
  }
  if (request.method === "GET" && request.url?.startsWith("/v1/runtime/runs")) {
    if (request.headers["x-api-key"] !== apiKey) {
      return json(response, { code: "unauthorized" }, 401);
    }
    const url = new URL(request.url, `http://127.0.0.1:${memoryPort}`);
    if (url.pathname === "/v1/runtime/runs") {
      requestCounts.runtimeList += 1;
      return json(response, {
        items: [runtimeRun()],
        total: 1,
        limit: Number(url.searchParams.get("limit") ?? 25),
        offset: Number(url.searchParams.get("offset") ?? 0),
      });
    }
    if (url.pathname === `/v1/runtime/runs/${runtimeRunId}`) {
      requestCounts.runtimeDetail += 1;
      return json(response, {
        run: runtimeRun(),
        transitions: [{
          id: "runtime-transition-1",
          run_id: runtimeRunId,
          step: 0,
          trace_id: "00000000-0000-7000-8000-000000000702",
          from_state: "observing",
          to_state: "recalling",
          input_refs: [],
          output_refs: [],
          attempt: 1,
          deadline_at_us: 1785556860000000,
          payload: { phase: "observe" },
          created_at_us: 1785556800100000,
        }],
      });
    }
    return json(response, { code: "not_found" }, 404);
  }
  if (request.method !== "POST" || !request.url?.startsWith("/v1/memory/")) {
    return json(response, { code: "not_found" }, 404);
  }
  if (request.headers["x-api-key"] !== apiKey) {
    return json(response, { code: "unauthorized" }, 401);
  }
  const envelope = JSON.parse(await readBody(request));
  const operation = request.url.split("/").at(-1);
  if (operation === "stats") {
    requestCounts.stats += 1;
    return command(response, envelope, "stats", {
      scope: envelope.access_scope,
      fragment_count: fragmentCount,
      event_count: eventCount,
    });
  }
  if (operation === "observe") {
    fragmentCount += 1;
    eventCount += 1;
    const eventId = fixtureId(200 + eventCount);
    storedFragments.push({
      fragment: envelope.command.payload.fragment,
      eventId,
      scopeKey: accessScopeKey(envelope.access_scope),
    });
    observeState(envelope.access_scope, envelope.command.payload.fragment, eventId);
    return command(response, envelope, "observe", {
      fragment_id: envelope.command.payload.fragment.id,
      event_id: eventId,
      sequence: eventCount,
      content_hash: "0".repeat(64),
      created: true,
      quarantined: false,
    });
  }
  if (operation === "commit") {
    const receipts = envelope.command.payload.fragments.map((fragment) => {
      fragmentCount += 1;
      eventCount += 1;
      const eventId = "00000000-0000-0000-0000-000000000202";
      storedFragments.push({ fragment, eventId, scopeKey: accessScopeKey(envelope.access_scope) });
      return {
        fragment_id: fragment.id,
        event_id: eventId,
        sequence: eventCount,
        content_hash: "1".repeat(64),
        created: true,
        quarantined: false,
      };
    });
    return command(response, envelope, "commit", { receipts });
  }
  if (operation === "correct" || operation === "forget") {
    const applyStateMutation = operation === "correct"
      ? prepareCorrectState(envelope.access_scope, envelope.command.payload)
      : prepareForgetState(envelope.access_scope, envelope.command.payload);
    if (!applyStateMutation) {
      return json(response, { code: "invalid_argument" }, 400);
    }
    fragmentCount += 1;
    eventCount += 1;
    const mutationFragmentId = fixtureId(500 + eventCount);
    const mutationEventId = fixtureId(600 + eventCount);
    applyStateMutation(mutationFragmentId, mutationEventId);
    return command(response, envelope, "mutation", {
      action: operation,
      target_fragment_id: envelope.command.payload.target_fragment_id,
      fragment_id: mutationFragmentId,
      event_id: mutationEventId,
      sequence: eventCount,
      relation: operation === "correct" ? "supersedes" : null,
    });
  }
  if (operation === "recall") {
    const statePacket = recallStates(envelope.access_scope, envelope.command.payload);
    if (statePacket) {
      return command(response, envelope, "recall", statePacket);
    }
    const matches = recalledFragments(envelope.command.payload.text);
    const evidence = matches.map(({ fragment, eventId }) => ({
      fragment_id: fragment.id,
      event_id: eventId,
      evidence_ref_id: null,
    }));
    return command(response, envelope, "recall", {
      query_id: "00000000-0000-0000-0000-000000000301",
      intent: envelope.command.payload.mode,
      current_states: [], historical_states: [], activated_surfaces: [],
      episodes: matches.map(({ fragment, eventId }, index) => ({
        event_id: eventId,
        fragment_id: fragment.id,
        occurred_at: fragment.occurred_at,
        summary: fragmentText(fragment),
        evidence: [evidence[index]],
      })),
      points: matches.map(({ fragment }) => ({
        point_id: fragment.id,
        kind: fragment.kind,
        confidence: fragment.confidence,
        validity: fragment.validity,
        source_fragments: [fragment.id],
      })),
      paths: matches.map(({ fragment }) => ({
        point_ids: [fragment.id],
        edge_ids: [],
        score: 1,
      })),
      procedures: [], constraints: [], preferences: [], past_failures: [], evidence, conflicts: [], missing_evidence: [],
      coverage: { seed_count: matches.length, visited_nodes: matches.length, max_depth: 0, evidence_count: evidence.length, missing_evidence_count: 0, truncated: false },
      confidence: matches.length > 0 ? 1 : 0,
      recommended_context: JSON.stringify(matches.map(({ fragment }) => fragmentText(fragment))),
    });
  }
  if (operation === "explain") {
    return command(response, envelope, "explain", {
      seeds: [], traversals: [], surfaces: [], inferred_edges: [], verified_fragments: [],
      limits: envelope.command.payload.limits,
      truncated: false,
    });
  }
  return json(response, { code: "invalid_argument" }, 400);
});

server.listen(memoryPort, "127.0.0.1");
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => server.close(() => process.exit(0)));
}

function command(response, request, operation, payload) {
  return json(response, {
    api_version: "v1",
    request_id: request.request_id,
    response: { response: operation, payload },
  });
}

function runtimeRun() {
  return {
    run_id: runtimeRunId,
    scope: {
      namespace_id: namespaceId,
      tenant_id: null,
      user_id: null,
      agent_id: null,
      project_id: null,
    },
    state: "completed",
    next_step: 1,
    version: 1,
    created_at_us: 1785556800000000,
    updated_at_us: 1785556801000000,
    control: null,
  };
}

function recalledFragments(query) {
  const normalized = String(query).trim().toLowerCase();
  return storedFragments.filter(({ fragment }) => {
    if (!["fact", "constraint", "preference", "failure", "success"].includes(fragment.kind)) {
      return false;
    }
    return fragmentText(fragment).toLowerCase().includes(normalized);
  });
}

function fragmentText(fragment) {
  return fragment.content?.type === "text"
    ? String(fragment.content.text)
    : JSON.stringify(fragment.content);
}

function observeState(scope, fragment, eventId) {
  const declaration = fragment.metadata?.state;
  if (!declaration) return;
  const chain = stateChain(scope, declaration.key);
  chain.versions.push({
    state_id: chain.stateId,
    version_id: fixtureId(400 + ++stateVersionCount),
    key: declaration.key,
    value: declaration.value,
    valid_from: fragment.occurred_at,
    valid_to: null,
    validity: fragment.validity,
    evidence: [{ fragment_id: fragment.id, event_id: eventId, evidence_ref_id: null }],
    reason: declaration.reason,
    source_event_id: eventId,
    sourceFragmentId: fragment.id,
  });
}

function prepareCorrectState(scope, payload) {
  const located = findStateVersion(scope, payload.target_fragment_id);
  if (!payload.state) {
    return located || hasFragmentInScope(scope, payload.target_fragment_id) ? () => {} : null;
  }
  if (!located || located.version.key !== payload.state.key || located.version.valid_to !== null) {
    return null;
  }
  return (fragmentId, eventId) => {
    located.version.valid_to = payload.occurred_at;
    located.version.validity = "superseded";
    located.chain.versions.push({
      state_id: located.chain.stateId,
      version_id: fixtureId(400 + ++stateVersionCount),
      key: payload.state.key,
      value: payload.state.value,
      valid_from: payload.occurred_at,
      valid_to: null,
      validity: "verified",
      evidence: [{ fragment_id: fragmentId, event_id: eventId, evidence_ref_id: null }],
      reason: payload.state.reason,
      source_event_id: eventId,
      sourceFragmentId: fragmentId,
    });
  };
}

function prepareForgetState(scope, payload) {
  const located = findStateVersion(scope, payload.target_fragment_id);
  if (located) {
    if (located.version.valid_to !== null) return null;
    return () => {
      located.version.valid_to = payload.occurred_at;
      located.version.validity = "invalidated";
    };
  }
  return hasFragmentInScope(scope, payload.target_fragment_id) ? () => {} : null;
}

function findStateVersion(scope, targetFragmentId) {
  const scopeKey = accessScopeKey(scope);
  for (const chain of stateChains.values()) {
    if (chain.scopeKey !== scopeKey) continue;
    const version = chain.versions.find((candidate) => candidate.sourceFragmentId === targetFragmentId);
    if (version) return { chain, version };
  }
  return null;
}

function hasFragmentInScope(scope, targetFragmentId) {
  const scopeKey = accessScopeKey(scope);
  return storedFragments.some(({ fragment, scopeKey: storedScopeKey }) =>
    fragment.id === targetFragmentId && storedScopeKey === scopeKey);
}

function recallStates(scope, payload) {
  if (payload.mode !== "current_state" && payload.mode !== "historical_state") return null;
  const chain = stateChains.get(`${accessScopeKey(scope)}:${payload.text}`);
  const versions = chain?.versions ?? [];
  const current = versions.filter((version) => version.valid_to === null).map(publicStateVersion);
  const historical = versions.filter((version) => version.valid_to !== null).map(publicStateVersion);
  const evidence = [...current, ...historical].flatMap((version) => version.evidence);
  return {
    query_id: fixtureId(700 + ++queryCount),
    intent: payload.mode,
    current_states: current,
    historical_states: historical,
    activated_surfaces: [], episodes: [], points: [], paths: [], procedures: [], constraints: [], preferences: [], past_failures: [],
    evidence,
    conflicts: [],
    missing_evidence: [],
    coverage: {
      seed_count: current.length,
      visited_nodes: current.length + historical.length,
      max_depth: 0,
      evidence_count: evidence.length,
      missing_evidence_count: 0,
      truncated: false,
    },
    confidence: current.length > 0 ? 1 : 0,
    recommended_context: JSON.stringify(current.map(({ key, value }) => ({ key, value }))),
  };
}

function stateChain(scope, key) {
  const mapKey = `${accessScopeKey(scope)}:${key}`;
  let chain = stateChains.get(mapKey);
  if (!chain) {
    chain = { scopeKey: accessScopeKey(scope), stateId: fixtureId(400 + ++stateCount), versions: [] };
    stateChains.set(mapKey, chain);
  }
  return chain;
}

function publicStateVersion({ sourceFragmentId: _sourceFragmentId, ...version }) {
  return version;
}

function accessScopeKey(scope) {
  return [scope.namespace_id, scope.tenant_id, scope.user_id, scope.agent_id, scope.project_id]
    .map((value) => value ?? "-")
    .join(":");
}

function fixtureId(value) {
  return `00000000-0000-0000-0000-${String(value).padStart(12, "0")}`;
}

function json(response, body, status = 200) {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}

function metricsText(sequence) {
  const total = sequence * 50;
  const success = sequence * 48;
  const rateLimited = sequence * 2;
  const bucket = sequence * 50;
  return [
    `jc_memory_requests_total{operation="recall",transport="http",outcome="success"} ${success}`,
    `jc_memory_requests_total{operation="recall",transport="http",outcome="rate_limited"} ${rateLimited}`,
    `jc_memory_requests_total{operation="recall",transport="http",outcome="error"} 0`,
    `jc_memory_requests_total{operation="recall",transport="http",outcome="timeout"} 0`,
    `jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="0.5"} ${bucket * 0.5}`,
    `jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="1"} ${bucket * 0.9}`,
    `jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="2"} ${bucket * 0.98}`,
    `jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="+Inf"} ${total}`,
    `jc_memory_request_duration_seconds_count{operation="recall",transport="http"} ${total}`,
    `jc_memory_request_duration_seconds_sum{operation="recall",transport="http"} ${total}`,
    `jc_memory_requests_in_flight ${sequence % 4}`,
    "jc_memory_server_ready 1",
    "",
  ].join("\n");
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk) => { body += chunk; });
    request.on("end", () => resolve(body));
    request.on("error", reject);
  });
}
