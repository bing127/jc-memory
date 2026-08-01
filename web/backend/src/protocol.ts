import { z } from "zod";

export const memoryOperations = [
  "stats",
  "observe",
  "recall",
  "explain",
  "correct",
  "forget",
  "commit",
] as const;

export type MemoryOperation = (typeof memoryOperations)[number];

export const uuidStringSchema = z.string().regex(
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
  "expected a UUID string",
);

export const accessScopeSchema = z.strictObject({
  namespace_id: uuidStringSchema,
  tenant_id: uuidStringSchema.nullable(),
  user_id: uuidStringSchema.nullable(),
  agent_id: uuidStringSchema.nullable(),
  project_id: uuidStringSchema.nullable(),
});

export type AccessScope = z.infer<typeof accessScopeSchema>;

export interface MemoryCommandInput {
  operation: MemoryOperation;
  requestId: string;
  accessScope: AccessScope;
  payload: Record<string, unknown>;
}

export interface MemoryResponseEnvelope {
  api_version: string;
  request_id: string;
  response: {
    response: string;
    payload: unknown;
  };
}

export const memoryResponseSchema = z.object({
  api_version: z.string(),
  request_id: z.string(),
  response: z.object({
    response: z.string(),
    payload: z.unknown(),
  }),
});

export function buildMemoryEnvelope(input: MemoryCommandInput) {
  return {
    api_version: "v1",
    request_id: input.requestId,
    access_scope: input.accessScope,
    command: {
      command: input.operation,
      payload: input.operation === "stats" ? null : input.payload,
    },
  };
}
