import { randomUUID } from "node:crypto";
import type { FastifyInstance } from "fastify";
import { z } from "zod";

import type { AuthService } from "../auth.js";
import type { InstanceStore } from "../instances.js";
import {
  uuidStringSchema,
  type AccessScope,
  type MemoryOperation,
} from "../protocol.js";
import type { UpstreamClient } from "../upstream.js";
import { requireAllowedOrigin, requireSession } from "./guards.js";

const paramsSchema = z.strictObject({
  instanceId: uuidStringSchema,
  operation: z.enum(["stats", "observe", "recall", "explain", "commit", "correct", "forget"]),
});

const scopeSchema = z.strictObject({
  tenantId: uuidStringSchema.nullable(),
  userId: uuidStringSchema.nullable(),
  agentId: uuidStringSchema.nullable(),
  projectId: uuidStringSchema.nullable(),
});

const baseBodySchema = z.strictObject({ scope: scopeSchema });
const observeBodySchema = baseBodySchema.extend({
  fragment: z.record(z.string(), z.unknown()),
});
const commitBodySchema = baseBodySchema.extend({
  fragments: z.array(z.record(z.string(), z.unknown())).min(1).max(20),
});
const fragmentContentSchema = z.discriminatedUnion("type", [
  z.strictObject({ type: z.literal("text"), text: z.string().trim().min(1).max(1_048_576) }),
  z.strictObject({ type: z.literal("json"), value: z.unknown() }),
  z.strictObject({
    type: z.literal("blob"),
    blob_id: z.string().regex(/^[0-9a-f]{64}$/i),
    media_type: z.string().trim().min(1).max(255),
    byte_len: z.number().int().nonnegative(),
  }),
]);
const mutationBodySchema = baseBodySchema.extend({
  targetFragmentId: uuidStringSchema,
  content: fragmentContentSchema,
  source: z.strictObject({
    sourceType: z.enum(["user", "agent", "model", "tool", "import", "system"]),
    sourceId: z.string().trim().min(1).max(512),
    actorId: z.string().max(512).nullable(),
    observerId: z.string().max(512).nullable(),
    toolId: z.string().max(512).nullable(),
    uri: z.string().max(4_096).nullable(),
  }),
  perspective: z.enum(["first_person", "second_person", "third_person", "system", "tool"]),
  occurredAt: z.iso.datetime(),
  verificationMethod: z.enum(["manual", "tool", "external_source", "cross_evidence"]),
  reason: z.string().trim().min(1).max(2_048),
});
const recallModeSchema = z.enum([
  "exact",
  "semantic",
  "temporal",
  "causal",
  "entity",
  "associative",
  "procedural",
  "current_state",
  "historical_state",
  "deep_reconstruction",
]);
const recallBodySchema = baseBodySchema.extend({
  mode: recallModeSchema,
  text: z.string().max(8_192),
  entities: z.array(z.string().min(1).max(256)).max(64),
  atTime: z.iso.datetime().nullable(),
  limits: z.strictObject({
    maxSeeds: z.number().int().min(1).max(1_024),
    maxNeighbors: z.number().int().min(1).max(1_024),
    maxHops: z.number().int().min(0).max(32),
    maxNodes: z.number().int().min(1).max(10_000),
    timeoutMs: z.number().int().min(1).max(30_000),
  }),
});

export async function registerMemoryRoutes(
  app: FastifyInstance,
  auth: AuthService,
  store: InstanceStore,
  upstream: UpstreamClient,
): Promise<void> {
  app.post(
    "/api/v1/instances/:instanceId/memory/:operation",
    async (request, reply) => {
      if (!requireSession(request, reply, auth) || !requireAllowedOrigin(request, reply, auth)) return;
      const params = paramsSchema.safeParse(request.params);
      if (!params.success) {
        return reply.code(400).send({ code: "invalid_argument", message: "实例或操作无效" });
      }
      const connection = store.getConnection(params.data.instanceId);
      if (!connection) {
        return reply.code(404).send({ code: "not_found", message: "实例连接不存在" });
      }
      const parsed = parseOperation(params.data.operation, request.body);
      if (!parsed) {
        return reply.code(400).send({ code: "invalid_argument", message: "命令输入格式无效" });
      }
      const accessScope = toAccessScope(connection.namespaceId, parsed.scope);
      const response = await upstream.execute(connection, {
        operation: params.data.operation,
        requestId: randomUUID(),
        accessScope,
        payload: bindPayloadToScope(params.data.operation, parsed.payload, accessScope),
      });
      return response;
    },
  );
}

function bindPayloadToScope(
  operation: MemoryOperation,
  payload: Record<string, unknown>,
  accessScope: AccessScope,
): Record<string, unknown> {
  if (operation === "observe") {
    const fragment = payload.fragment as Record<string, unknown>;
    return {
      fragment: {
        ...fragment,
        scope: accessScope,
      },
    };
  }
  if (operation === "commit") {
    return {
      fragments: (payload.fragments as Record<string, unknown>[]).map((fragment) => ({
        ...fragment,
        scope: accessScope,
      })),
    };
  }
  return payload;
}

function parseOperation(
  operation: MemoryOperation,
  body: unknown,
): { scope: z.infer<typeof scopeSchema>; payload: Record<string, unknown> } | null {
  if (operation === "stats") {
    const parsed = baseBodySchema.safeParse(body);
    return parsed.success ? { scope: parsed.data.scope, payload: {} } : null;
  }
  if (operation === "observe") {
    const parsed = observeBodySchema.safeParse(body);
    return parsed.success
      ? { scope: parsed.data.scope, payload: { fragment: parsed.data.fragment } }
      : null;
  }
  if (operation === "commit") {
    const parsed = commitBodySchema.safeParse(body);
    return parsed.success
      ? { scope: parsed.data.scope, payload: { fragments: parsed.data.fragments } }
      : null;
  }
  if (operation === "correct" || operation === "forget") {
    const parsed = mutationBodySchema.safeParse(body);
    if (!parsed.success) return null;
    return {
      scope: parsed.data.scope,
      payload: {
        target_fragment_id: parsed.data.targetFragmentId,
        content: parsed.data.content,
        source: {
          source_type: parsed.data.source.sourceType,
          source_id: parsed.data.source.sourceId,
          actor_id: parsed.data.source.actorId,
          observer_id: parsed.data.source.observerId,
          tool_id: parsed.data.source.toolId,
          uri: parsed.data.source.uri,
        },
        perspective: parsed.data.perspective,
        occurred_at: parsed.data.occurredAt,
        verification_method: parsed.data.verificationMethod,
        reason: parsed.data.reason,
      },
    };
  }
  const parsed = recallBodySchema.safeParse(body);
  if (!parsed.success) return null;
  return {
    scope: parsed.data.scope,
    payload: {
      mode: parsed.data.mode,
      text: parsed.data.text,
      entities: parsed.data.entities,
      at_time: parsed.data.atTime,
      limits: {
        max_seeds: parsed.data.limits.maxSeeds,
        max_neighbors: parsed.data.limits.maxNeighbors,
        max_hops: parsed.data.limits.maxHops,
        max_nodes: parsed.data.limits.maxNodes,
        timeout_ms: parsed.data.limits.timeoutMs,
      },
    },
  };
}

function toAccessScope(
  namespaceId: string,
  scope: z.infer<typeof scopeSchema>,
): AccessScope {
  return {
    namespace_id: namespaceId,
    tenant_id: scope.tenantId,
    user_id: scope.userId,
    agent_id: scope.agentId,
    project_id: scope.projectId,
  };
}
