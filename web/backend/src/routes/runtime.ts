import type { FastifyInstance } from "fastify";
import { z } from "zod";

import type { AuthService } from "../auth.js";
import type { InstanceStore } from "../instances.js";
import { uuidStringSchema } from "../protocol.js";
import type { UpstreamClient } from "../upstream.js";
import { requireAllowedOrigin, requireSession } from "./guards.js";

const instanceParamsSchema = z.strictObject({ instanceId: uuidStringSchema });
const detailParamsSchema = z.strictObject({
  instanceId: uuidStringSchema,
  runId: uuidStringSchema,
});
const runtimeStateSchema = z.enum([
  "observing",
  "recalling",
  "reasoning",
  "awaiting_approval",
  "executing",
  "committing",
  "waiting_user",
  "paused",
  "completed",
  "failed",
  "cancelled",
]);
const listQuerySchema = z.strictObject({
  state: runtimeStateSchema.optional(),
  limit: z.coerce.number().int().min(1).max(100).default(25),
  offset: z.coerce.number().int().min(0).max(1_000_000).default(0),
});
const startBodySchema = z.strictObject({ prompt: z.string().trim().min(1).max(32_768) });

export async function registerRuntimeRoutes(
  app: FastifyInstance,
  auth: AuthService,
  store: InstanceStore,
  upstream: UpstreamClient,
): Promise<void> {
  app.post("/api/v1/instances/:instanceId/runtime/runs", async (request, reply) => {
    if (!requireSession(request, reply, auth) || !requireAllowedOrigin(request, reply, auth)) return;
    const params = instanceParamsSchema.safeParse(request.params);
    const body = startBodySchema.safeParse(request.body);
    if (!params.success || !body.success) {
      return reply.code(400).send({ code: "invalid_argument", message: "Runtime Prompt 无效" });
    }
    const connection = store.getConnection(params.data.instanceId);
    if (!connection) {
      return reply.code(404).send({ code: "not_found", message: "实例连接不存在" });
    }
    const result = await upstream.runtimeStart(connection, body.data.prompt);
    return reply.code(201).send(result);
  });

  app.get("/api/v1/instances/:instanceId/runtime/runs", async (request, reply) => {
    if (!requireSession(request, reply, auth)) return;
    const params = instanceParamsSchema.safeParse(request.params);
    const query = listQuerySchema.safeParse(request.query);
    if (!params.success || !query.success) {
      return reply.code(400).send({ code: "invalid_argument", message: "Runtime 查询参数无效" });
    }
    const connection = store.getConnection(params.data.instanceId);
    if (!connection) {
      return reply.code(404).send({ code: "not_found", message: "实例连接不存在" });
    }
    return upstream.runtimeRuns(connection, query.data);
  });

  app.get(
    "/api/v1/instances/:instanceId/runtime/runs/:runId",
    async (request, reply) => {
      if (!requireSession(request, reply, auth)) return;
      const params = detailParamsSchema.safeParse(request.params);
      if (!params.success) {
        return reply.code(400).send({ code: "invalid_argument", message: "Runtime Run ID 无效" });
      }
      const connection = store.getConnection(params.data.instanceId);
      if (!connection) {
        return reply.code(404).send({ code: "not_found", message: "实例连接不存在" });
      }
      return upstream.runtimeRun(connection, params.data.runId);
    },
  );
}
