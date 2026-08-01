import { createServer } from "node:http";

const namespaceId = "00000000-0000-0000-0000-000000000001";
const apiKey = "sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t";
const runtimeRunId = "00000000-0000-7000-8000-000000000701";
const memoryPort = Number(process.env.JC_WEB_E2E_MEMORY_PORT ?? 8787);
let fragmentCount = 0;
let eventCount = 0;
const storedFragments = [];

const server = createServer(async (request, response) => {
  if (request.method === "GET" && request.url === "/healthz") return json(response, { status: "ok" });
  if (request.method === "GET" && request.url === "/readyz") return json(response, { status: "ready" });
  if (request.method === "GET" && request.url === "/metrics") {
    response.writeHead(200, { "content-type": "text/plain; version=0.0.4" });
    return response.end("jc_memory_server_ready 1\n");
  }
  if (request.method === "POST" && request.url === "/v1/runtime/runs") {
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
      return json(response, {
        items: [runtimeRun()],
        total: 1,
        limit: Number(url.searchParams.get("limit") ?? 25),
        offset: Number(url.searchParams.get("offset") ?? 0),
      });
    }
    if (url.pathname === `/v1/runtime/runs/${runtimeRunId}`) {
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
    return command(response, envelope, "stats", {
      scope: envelope.access_scope,
      fragment_count: fragmentCount,
      event_count: eventCount,
    });
  }
  if (operation === "observe") {
    fragmentCount += 1;
    eventCount += 1;
    const eventId = "00000000-0000-0000-0000-000000000201";
    storedFragments.push({ fragment: envelope.command.payload.fragment, eventId });
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
      storedFragments.push({ fragment, eventId });
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
    fragmentCount += 1;
    eventCount += 1;
    return command(response, envelope, "mutation", {
      action: operation,
      target_fragment_id: envelope.command.payload.target_fragment_id,
      fragment_id: operation === "correct"
        ? "00000000-0000-0000-0000-000000000501"
        : "00000000-0000-0000-0000-000000000502",
      event_id: operation === "correct"
        ? "00000000-0000-0000-0000-000000000601"
        : "00000000-0000-0000-0000-000000000602",
      sequence: eventCount,
      relation: operation === "correct" ? "supersedes" : null,
    });
  }
  if (operation === "recall") {
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

function json(response, body, status = 200) {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
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
