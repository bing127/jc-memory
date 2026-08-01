import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const namespaceId = "00000000-0000-0000-0000-000000000001";
const apiKey = "fixture-instance-api-key";
const providerKey = "fixture-provider-api-key";
const memoryPort = Number(process.env.JC_WEB_E2E_MEMORY_PORT ?? 8787);
const grpcPort = Number(process.env.JC_WEB_E2E_GRPC_PORT ?? 8788);
const providerPort = Number(process.env.JC_WEB_E2E_PROVIDER_PORT ?? 8789);
const webRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const repositoryRoot = dirname(webRoot);
const dataDirectory = await mkdtemp(join(tmpdir(), "jc-memory-web-e2e-"));

const provider = createServer(async (request, response) => {
  if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
    return json(response, { error: "not_found" }, 404);
  }
  if (request.headers.authorization !== `Bearer ${providerKey}`) {
    return json(response, { error: "unauthorized" }, 401);
  }
  await readBody(request);
  return json(response, {
    id: "fixture-chat-completion",
    object: "chat.completion",
    choices: [{
      index: 0,
      message: {
        role: "assistant",
        content: JSON.stringify({
          kind: "final_response",
          payload: { text: "Fixture FinalResponse" },
        }),
      },
      finish_reason: "stop",
    }],
  });
});
await listen(provider, providerPort);

const server = spawn("cargo", ["run", "-p", "jc-memory-server"], {
  cwd: repositoryRoot,
  env: {
    ...process.env,
    CARGO_INCREMENTAL: "0",
    JC_MEMORY_BIND: `127.0.0.1:${memoryPort}`,
    JC_MEMORY_GRPC_BIND: `127.0.0.1:${grpcPort}`,
    JC_MEMORY_DATA_DIR: dataDirectory,
    JC_MEMORY_API_KEY: apiKey,
    JC_MEMORY_NAMESPACE_ID: namespaceId,
    JC_MEMORY_RUNTIME_PROVIDER_ENDPOINT: `http://127.0.0.1:${providerPort}/v1/chat/completions`,
    JC_MEMORY_RUNTIME_PROVIDER_MODEL: "fixture-model",
    JC_MEMORY_RUNTIME_PROVIDER_API_KEY: providerKey,
  },
  stdio: "inherit",
});

let stopping = false;
server.once("error", (error) => {
  console.error(error);
  void stop(1, false);
});
server.once("exit", (code, signal) => {
  if (!stopping) {
    console.error(`真实 memory-server 提前退出：${signal ?? code ?? "unknown"}`);
    void stop(code ?? 1, false);
  }
});
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => void stop(0, true));
}

async function stop(exitCode, terminateServer) {
  if (stopping) return;
  stopping = true;
  if (terminateServer && server.exitCode === null && server.signalCode === null) {
    server.kill("SIGINT");
    await new Promise((resolve) => server.once("exit", resolve));
  }
  await new Promise((resolve) => provider.close(() => resolve()));
  await rm(dataDirectory, { recursive: true, force: true });
  process.exit(exitCode);
}

function listen(httpServer, port) {
  return new Promise((resolve, reject) => {
    httpServer.once("error", reject);
    httpServer.listen(port, "127.0.0.1", resolve);
  });
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

function json(response, body, status = 200) {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}
