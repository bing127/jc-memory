import { createServer, type Server } from "node:http";
import { readFile } from "node:fs/promises";
import { afterEach, describe, expect, it } from "vitest";
import { EmbeddedMemoryClient, createNapiEmbeddedClient } from "../embedded.js";
import { RemoteMemoryClient } from "../remote.js";
import type { MemoryResponse, RecallRequest } from "../types.js";

type Fixture = {
  request: RecallRequest;
  response: Record<string, unknown>;
};

const servers: Server[] = [];

afterEach(async () => {
  await Promise.all(
    servers.splice(0).map(
      (server) =>
        new Promise<void>((resolve, reject) => {
          server.close((error) => (error ? reject(error) : resolve()));
        }),
    ),
  );
});

describe("RemoteMemoryClient", () => {
  it("returns the canonical recall packet", async () => {
    const fixture = JSON.parse(
      await readFile("../contracts/fixtures/recall-exact.json", "utf8"),
    ) as Fixture;
    let authorization = "";
    let body = "";
    const server = createServer((request, response) => {
      authorization = request.headers.authorization ?? "";
      request.setEncoding("utf8");
      request.on("data", (chunk) => {
        body += chunk;
      });
      request.on("end", () => {
        response.setHeader("content-type", "application/json");
        response.end(JSON.stringify(fixture.response));
      });
    });
    servers.push(server);
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", () => resolve()));
    const address = server.address();
    if (!address || typeof address === "string") {
      throw new Error("测试服务器未分配端口");
    }

    const client = new RemoteMemoryClient({
      baseUrl: `http://127.0.0.1:${address.port}`,
      apiKey: "fixture",
    });
    const response = await client.recall(fixture.request);

    expect(response).toEqual(fixture.response);
    expect(authorization).toBe("Bearer fixture");
    expect(JSON.parse(body)).toEqual(fixture.request);
  });

  it("delegates embedded commands and rejects calls after close", async () => {
    const request = {
      api_version: "v1",
      request_id: "embedded-1",
      access_scope: {
        namespace_id: "namespace",
        tenant_id: null,
        user_id: null,
        agent_id: null,
        project_id: null,
      },
      command: {
        command: "stats",
        payload: null,
      },
    } as const;
    const expected = {
      api_version: "v1",
      request_id: "embedded-1",
      response: { response: "stats", payload: {} },
    } satisfies MemoryResponse;
    const seen: unknown[] = [];
    const client = new EmbeddedMemoryClient(async (value) => {
      seen.push(value);
      return expected;
    });

    await expect(client.stats(request)).resolves.toEqual(expected);
    expect(seen).toEqual([request]);
    client.close();
    await expect(client.stats(request)).rejects.toMatchObject({ code: "internal" });
  });

  it("maps server errors to a stable code and request id", async () => {
    const fixture = JSON.parse(
      await readFile("../contracts/fixtures/error-invalid-argument.json", "utf8"),
    ) as Fixture;
    const server = createServer((_request, response) => {
      response.statusCode = 400;
      response.setHeader("content-type", "application/json");
      response.end(
        JSON.stringify({
          code: "invalid_argument",
          message: "request_id is invalid",
          request_id: fixture.request.request_id,
        }),
      );
    });
    servers.push(server);
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", () => resolve()));
    const address = server.address();
    if (!address || typeof address === "string") {
      throw new Error("测试服务器未分配端口");
    }

    const client = new RemoteMemoryClient({
      baseUrl: `http://127.0.0.1:${address.port}`,
      apiKey: "fixture",
    });
    await expect(client.recall(fixture.request)).rejects.toMatchObject({
      code: "invalid_argument",
      requestId: fixture.request.request_id,
      status: 400,
    });
  });

  it("maps caller cancellation to a stable cancelled error", async () => {
    const fixture = JSON.parse(
      await readFile("../contracts/fixtures/recall-exact.json", "utf8"),
    ) as Fixture;
    const client = new RemoteMemoryClient({
      baseUrl: "http://fixture.invalid",
      fetch: async (_input, init) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener(
            "abort",
            () => reject(new DOMException("aborted", "AbortError")),
            { once: true },
          );
        }),
    });
    const controller = new AbortController();
    const pending = client.recall(fixture.request, { signal: controller.signal });
    controller.abort();

    await expect(pending).rejects.toMatchObject({
      code: "cancelled",
      requestId: fixture.request.request_id,
    });
  });

  it("adapts the napi JSON boundary to the typed embedded client", async () => {
    const fixture = JSON.parse(
      await readFile("../contracts/fixtures/recall-exact.json", "utf8"),
    ) as Fixture;
    let closed = false;
    const client = createNapiEmbeddedClient({
      execute: async (requestJson) => {
        expect(JSON.parse(requestJson)).toEqual(fixture.request);
        return JSON.stringify(fixture.response);
      },
      close: async () => {
        closed = true;
      },
    });

    await expect(client.recall(fixture.request)).resolves.toEqual(fixture.response);
    await client.close();
    expect(closed).toBe(true);
  });
});
