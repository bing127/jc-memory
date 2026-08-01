export interface Session {
  authenticated: true;
  subject: "owner";
}

export interface InstanceSummary {
  id: string;
  name: string;
  baseUrl: string;
  namespaceId: string;
  protocolVersion: "v1";
  createdAt: string;
  updatedAt: string;
}

export interface ScopeSelector {
  tenantId: string | null;
  userId: string | null;
  agentId: string | null;
  projectId: string | null;
}

export interface InstanceStatus {
  health: { status?: string };
  ready: { status?: string };
  serverReady: boolean;
}

export interface ApiErrorBody {
  code: string;
  message: string;
  requestId?: string;
  status?: number;
}

export type RuntimeState =
  | "observing"
  | "recalling"
  | "reasoning"
  | "awaiting_approval"
  | "executing"
  | "committing"
  | "waiting_user"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export interface RuntimeControlRecord {
  state: "running" | "waiting_user" | "paused" | "cancelled";
  resume_state: RuntimeState;
  user_input: string | null;
  next_attempt_at_us: number | null;
  attempt: number;
  updated_at_us: number;
}

export interface RuntimeRunRecord {
  run_id: string;
  scope: {
    namespace_id: string;
    tenant_id: string | null;
    user_id: string | null;
    agent_id: string | null;
    project_id: string | null;
  };
  state: RuntimeState;
  next_step: number;
  version: number;
  created_at_us: number;
  updated_at_us: number;
  control: RuntimeControlRecord | null;
}

export interface RuntimeRunPage {
  items: RuntimeRunRecord[];
  total: number;
  limit: number;
  offset: number;
}

export interface RuntimeTransitionRecord {
  id: string;
  run_id: string;
  step: number;
  trace_id: string;
  from_state: RuntimeState;
  to_state: RuntimeState;
  input_refs: unknown[];
  output_refs: unknown[];
  attempt: number;
  deadline_at_us: number;
  payload: unknown;
  created_at_us: number;
}

export interface RuntimeRunDetail {
  run: RuntimeRunRecord;
  transitions: RuntimeTransitionRecord[];
}

export interface RuntimeStartResult {
  outcome: "completed" | "awaiting_approval";
  run_id: string;
  state: RuntimeState;
  response: { text: string } | null;
  approval_id: string | null;
}
