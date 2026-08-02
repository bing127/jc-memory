import type { FastifyInstance } from "fastify";
import type WebSocket from "ws";
import { z } from "zod";

import type { AuthService } from "../auth.js";
import { attachInstanceStream } from "../instance-stream.js";
import type { InstanceStore } from "../instances.js";
import { parseServerMetrics } from "../metrics.js";
import { uuidStringSchema } from "../protocol.js";
import type { UpstreamClient } from "../upstream.js";
import { validateBaseUrl } from "../upstream.js";
import { requireAllowedOrigin, requireSession } from "./guards.js";

const instanceInputSchema = z.strictObject({
  name: z.string().trim().min(1).max(80),
  baseUrl: z.url().max(2_048),
  namespaceId: uuidStringSchema,
  apiKey: z.string().regex(/^sk-jcmemory-[a-z0-9]{34}$/),
});

const instanceParamsSchema = z.strictObject({ id: uuidStringSchema });

export async function registerInstanceRoutes(
  app: FastifyInstance,
  auth: AuthService,
  store: InstanceStore,
  upstream: UpstreamClient,
): Promise<void> {
  app.get("/api/v1/instances", async (request, reply) => {
    if (!requireSession(request, reply, auth)) return;
    return store.list();
  });

  app.post("/api/v1/instances", async (request, reply) => {
    if (!requireSession(request, reply, auth) || !requireAllowedOrigin(request, reply, auth)) return;
    const parsed = instanceInputSchema.safeParse(request.body);
    if (!parsed.success) {
      return reply.code(400).send({ code: "invalid_argument", message: "实例配置格式无效" });
    }
    let baseUrl: string;
    try {
      baseUrl = validateBaseUrl(parsed.data.baseUrl).href;
    } catch (error) {
      return reply.code(400).send({
        code: "invalid_argument",
        message: error instanceof Error ? error.message : "实例地址无效",
      });
    }
    const created = store.create({ ...parsed.data, baseUrl });
    return reply.code(201).send(created);
  });

  app.delete("/api/v1/instances/:id", async (request, reply) => {
    if (!requireSession(request, reply, auth) || !requireAllowedOrigin(request, reply, auth)) return;
    const parsed = instanceParamsSchema.safeParse(request.params);
    if (!parsed.success) {
      return reply.code(400).send({ code: "invalid_argument", message: "实例 ID 无效" });
    }
    if (!store.remove(parsed.data.id)) {
      return reply.code(404).send({ code: "not_found", message: "实例连接不存在" });
    }
    return reply.code(204).send();
  });

  app.get("/api/v1/instances/:id/status", async (request, reply) => {
    if (!requireSession(request, reply, auth)) return;
    const parsed = instanceParamsSchema.safeParse(request.params);
    if (!parsed.success) {
      return reply.code(400).send({ code: "invalid_argument", message: "实例 ID 无效" });
    }
    const connection = store.getConnection(parsed.data.id);
    if (!connection) {
      return reply.code(404).send({ code: "not_found", message: "实例连接不存在" });
    }
    const status = await upstream.status(connection);
    return {
      health: status.health,
      ready: status.ready,
      serverReady: /^jc_memory_server_ready 1$/m.test(status.metrics),
    };
  });

  app.get("/api/v1/instances/:id/metrics", async (request, reply) => {
    if (!requireSession(request, reply, auth)) return;
    const parsed = instanceParamsSchema.safeParse(request.params);
    if (!parsed.success) {
      return reply.code(400).send({ code: "invalid_argument", message: "实例 ID 无效" });
    }
    const connection = store.getConnection(parsed.data.id);
    if (!connection) {
      return reply.code(404).send({ code: "not_found", message: "实例连接不存在" });
    }
    const rawMetrics = await upstream.metrics(connection);
    return parseServerMetrics(rawMetrics);
  });

  app.get(
    "/api/v1/instances/:id/stream",
    { websocket: true },
    (socket: WebSocket, request) => {
      if (!auth.sessionFromRequest(request) || !auth.isAllowedOrigin(request.headers.origin)) {
        socket.close(1008, "unauthorized");
        return;
      }
      const params = instanceParamsSchema.safeParse(request.params);
      const connection = params.success ? store.getConnection(params.data.id) : null;
      if (!connection) {
        socket.close(1008, "instance_not_found");
        return;
      }
      attachInstanceStream(socket, connection, upstream);
    },
  );
}
