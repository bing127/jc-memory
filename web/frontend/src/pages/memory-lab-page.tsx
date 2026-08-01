import * as Tabs from "@radix-ui/react-tabs";
import { useMutation } from "@tanstack/react-query";
import { Braces, Eye, FileCheck2, PencilLine, Radio, Send, ShieldX } from "lucide-react";
import { useState, type FormEvent } from "react";

import { JsonViewer } from "../components/json-viewer.js";
import { MemoryPacket, type MemoryPacketData } from "../components/memory-packet.js";
import { Button } from "../components/ui/button.js";
import { Input } from "../components/ui/input.js";
import { api, ApiError } from "../lib/api.js";
import type { InstanceSummary, ScopeSelector } from "../lib/contracts.js";

const emptyScope: ScopeSelector = { tenantId: null, userId: null, agentId: null, projectId: null };
const recallModes = ["exact", "semantic", "temporal", "causal", "entity", "associative", "procedural", "current_state", "historical_state", "deep_reconstruction"] as const;

interface MemoryEnvelope {
  api_version: string;
  request_id: string;
  response: { response: string; payload: unknown };
}

type MemoryLabOperation = "observe" | "recall" | "explain" | "commit" | "correct" | "forget";

export function MemoryLabPage({ instance }: { instance: InstanceSummary }) {
  const [result, setResult] = useState<MemoryEnvelope | null>(null);
  const mutation = useMutation({
    mutationFn: ({ operation, body }: { operation: MemoryLabOperation; body: { scope: ScopeSelector } & Record<string, unknown> }) =>
      api.memory<MemoryEnvelope>(instance.id, operation, body),
    onSuccess: setResult,
  });

  return (
    <main className="console-page">
      <header className="page-header">
        <div><p className="page-kicker">MEMORY WORKSPACE</p><h1>Memory Lab</h1></div>
        <p>所有输入直接映射稳定协议；写操作失败后不会自动重试。</p>
      </header>
      <div className="authority-boundary">
        原始 Fragment、Event 和 EvidenceRef 不会被覆盖；Correct 与 Forget 只追加带审计证据的新记录。
      </div>
      <Tabs.Root defaultValue="observe" className="lab-tabs" onValueChange={() => { setResult(null); mutation.reset(); }}>
        <Tabs.List aria-label="Memory 操作">
          <Tabs.Trigger value="observe"><Send aria-hidden="true" />Observe</Tabs.Trigger>
          <Tabs.Trigger value="commit"><FileCheck2 aria-hidden="true" />Commit</Tabs.Trigger>
          <Tabs.Trigger value="recall"><Radio aria-hidden="true" />Recall</Tabs.Trigger>
          <Tabs.Trigger value="explain"><Eye aria-hidden="true" />Explain</Tabs.Trigger>
          <Tabs.Trigger value="correct"><PencilLine aria-hidden="true" />Correct</Tabs.Trigger>
          <Tabs.Trigger value="forget"><ShieldX aria-hidden="true" />Forget</Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="observe"><ObserveForm pending={mutation.isPending} onSubmit={(body) => mutation.mutate({ operation: "observe", body })} /></Tabs.Content>
        <Tabs.Content value="commit"><CommitForm pending={mutation.isPending} onSubmit={(body) => mutation.mutate({ operation: "commit", body })} /></Tabs.Content>
        <Tabs.Content value="recall"><RecallForm operation="recall" pending={mutation.isPending} onSubmit={(body) => mutation.mutate({ operation: "recall", body })} /></Tabs.Content>
        <Tabs.Content value="explain"><RecallForm operation="explain" pending={mutation.isPending} onSubmit={(body) => mutation.mutate({ operation: "explain", body })} /></Tabs.Content>
        <Tabs.Content value="correct"><MutationForm operation="correct" pending={mutation.isPending} onSubmit={(body) => mutation.mutate({ operation: "correct", body })} /></Tabs.Content>
        <Tabs.Content value="forget"><MutationForm operation="forget" pending={mutation.isPending} onSubmit={(body) => mutation.mutate({ operation: "forget", body })} /></Tabs.Content>
      </Tabs.Root>
      {mutation.error ? <div className="error-band">{mutation.error instanceof ApiError ? mutation.error.message : "Memory 操作失败"}</div> : null}
      {result ? <ResultView result={result} /> : <div className="lab-empty"><Braces aria-hidden="true" /><span>运行命令后在此查看结构化结果与 Canonical JSON</span></div>}
    </main>
  );
}

function ObserveForm({ pending, onSubmit }: { pending: boolean; onSubmit(body: { scope: ScopeSelector; fragment: Record<string, unknown> }): void }) {
  const [text, setText] = useState("");
  const [kind, setKind] = useState("fact");
  const [sourceId, setSourceId] = useState("control-plane-user");
  function submit(event: FormEvent) {
    event.preventDefault();
    onSubmit({
      scope: emptyScope,
      fragment: buildTextFragment(text, kind, sourceId),
    });
  }
  return (
    <form className="lab-form" onSubmit={submit}>
      <div className="form-grid">
        <label className="form-field"><span>Fragment Kind</span><select value={kind} onChange={(e) => setKind(e.target.value)}>{["observation", "claim", "fact", "constraint", "preference", "failure", "success"].map((value) => <option key={value}>{value}</option>)}</select></label>
        <label className="form-field"><span>Source ID</span><Input value={sourceId} onChange={(e) => setSourceId(e.target.value)} required /></label>
      </div>
      <label className="form-field"><span>原始文本</span><textarea value={text} onChange={(e) => setText(e.target.value)} rows={7} required /></label>
      <Button type="submit" disabled={pending}>{pending ? "正在写入" : "Observe Fragment"}<Send aria-hidden="true" size={16} /></Button>
    </form>
  );
}

function CommitForm({ pending, onSubmit }: { pending: boolean; onSubmit(body: { scope: ScopeSelector; fragments: Record<string, unknown>[] }): void }) {
  const [text, setText] = useState("");
  const [kind, setKind] = useState("fact");
  const [sourceId, setSourceId] = useState("control-plane-user");
  function submit(event: FormEvent) {
    event.preventDefault();
    onSubmit({ scope: emptyScope, fragments: [buildTextFragment(text, kind, sourceId)] });
  }
  return (
    <form className="lab-form" onSubmit={submit}>
      <div className="form-grid">
        <label className="form-field"><span>Fragment Kind</span><select value={kind} onChange={(event) => setKind(event.target.value)}>{["observation", "claim", "fact", "constraint", "preference", "failure", "success"].map((value) => <option key={value}>{value}</option>)}</select></label>
        <label className="form-field"><span>Source ID</span><Input value={sourceId} onChange={(event) => setSourceId(event.target.value)} required /></label>
      </div>
      <label className="form-field"><span>提交文本</span><textarea value={text} onChange={(event) => setText(event.target.value)} rows={7} required /></label>
      <p className="operation-note">一次 Commit 以原始顺序追加完整 Fragment；失败后不会自动重试。</p>
      <Button type="submit" disabled={pending}>{pending ? "正在提交" : "Commit Fragment"}<FileCheck2 aria-hidden="true" size={16} /></Button>
    </form>
  );
}

function MutationForm({
  operation,
  pending,
  onSubmit,
}: {
  operation: "correct" | "forget";
  pending: boolean;
  onSubmit(body: { scope: ScopeSelector } & Record<string, unknown>): void;
}) {
  const [targetFragmentId, setTargetFragmentId] = useState("");
  const [content, setContent] = useState(operation === "forget" ? "该记录已明确失效" : "");
  const [sourceId, setSourceId] = useState("control-plane-owner");
  const [reason, setReason] = useState("");
  const [verificationMethod, setVerificationMethod] = useState("manual");
  const [confirmation, setConfirmation] = useState("");
  const isForget = operation === "forget";
  function submit(event: FormEvent) {
    event.preventDefault();
    if (isForget && confirmation !== targetFragmentId) return;
    onSubmit({
      scope: emptyScope,
      targetFragmentId,
      content: { type: "text", text: content },
      source: {
        sourceType: "user",
        sourceId,
        actorId: null,
        observerId: null,
        toolId: null,
        uri: null,
      },
      perspective: "first_person",
      occurredAt: new Date().toISOString(),
      verificationMethod,
      reason,
    });
  }
  return (
    <form className={`lab-form ${isForget ? "danger-form" : ""}`} onSubmit={submit}>
      <div className="form-grid">
        <label className="form-field"><span>目标 Fragment ID</span><Input value={targetFragmentId} onChange={(event) => setTargetFragmentId(event.target.value)} pattern="[0-9a-fA-F-]{36}" required /></label>
        <label className="form-field"><span>验证方法</span><select value={verificationMethod} onChange={(event) => setVerificationMethod(event.target.value)}><option value="manual">manual</option><option value="tool">tool</option><option value="external_source">external_source</option><option value="cross_evidence">cross_evidence</option></select></label>
      </div>
      <div className="form-grid">
        <label className="form-field"><span>Source ID</span><Input value={sourceId} onChange={(event) => setSourceId(event.target.value)} required /></label>
        <label className="form-field"><span>{isForget ? "失效原因" : "修正原因"}</span><Input value={reason} onChange={(event) => setReason(event.target.value)} required /></label>
      </div>
      <label className="form-field"><span>{isForget ? "失效说明" : "修正内容"}</span><textarea value={content} onChange={(event) => setContent(event.target.value)} rows={5} required /></label>
      {isForget ? (
        <>
          <p className="operation-note danger-note">Forget 只追加 Invalidated 记录，不执行 Hard Delete，也不会移除原始证据。</p>
          <label className="form-field"><span>再次输入目标 Fragment ID</span><Input value={confirmation} onChange={(event) => setConfirmation(event.target.value)} required /></label>
        </>
      ) : (
        <p className="operation-note">Correct 追加 Correction Fragment，并通过 EvidenceRef 引用原始记录。</p>
      )}
      <Button type="submit" variant={isForget ? "danger" : "primary"} disabled={pending || (isForget && confirmation !== targetFragmentId)}>
        {pending ? "正在写入" : isForget ? "确认追加失效记录" : "提交修正"}
        {isForget ? <ShieldX aria-hidden="true" size={16} /> : <PencilLine aria-hidden="true" size={16} />}
      </Button>
    </form>
  );
}

function buildTextFragment(text: string, kind: string, sourceId: string): Record<string, unknown> {
  const now = new Date().toISOString();
  return {
    id: crypto.randomUUID(),
    session_id: null,
    trace_id: null,
    kind,
    content: { type: "text", text },
    source: { source_type: "user", source_id: sourceId, actor_id: null, observer_id: null, tool_id: null, uri: null },
    perspective: "first_person",
    occurred_at: now,
    observed_at: now,
    confidence: 1,
    importance: 0.5,
    validity: "unverified",
    security_label: "internal",
    evidence: [],
    metadata: {},
  };
}

function RecallForm({ operation, pending, onSubmit }: { operation: "recall" | "explain"; pending: boolean; onSubmit(body: { scope: ScopeSelector } & Record<string, unknown>): void }) {
  const [mode, setMode] = useState<(typeof recallModes)[number]>("semantic");
  const [text, setText] = useState("");
  const [entities, setEntities] = useState("");
  const [limits, setLimits] = useState({ maxSeeds: 8, maxNeighbors: 16, maxHops: 3, maxNodes: 64, timeoutMs: 2000 });
  function submit(event: FormEvent) {
    event.preventDefault();
    onSubmit({ scope: emptyScope, mode, text, entities: entities.split(",").map((item) => item.trim()).filter(Boolean), atTime: null, limits });
  }
  return (
    <form className="lab-form" onSubmit={submit}>
      <div className="form-grid">
        <label className="form-field"><span>Recall Mode</span><select value={mode} onChange={(e) => setMode(e.target.value as typeof mode)}>{recallModes.map((value) => <option key={value}>{value}</option>)}</select></label>
        <label className="form-field"><span>Entities（逗号分隔）</span><Input value={entities} onChange={(e) => setEntities(e.target.value)} /></label>
      </div>
      <label className="form-field"><span>Query Text</span><textarea value={text} onChange={(e) => setText(e.target.value)} rows={4} required /></label>
      <div className="limit-grid">
        {Object.entries(limits).map(([key, value]) => (
          <label className="form-field" key={key}><span>{key}</span><Input type="number" min={key === "maxHops" ? 0 : 1} value={value} onChange={(e) => setLimits((current) => ({ ...current, [key]: Number(e.target.value) }))} /></label>
        ))}
      </div>
      <Button type="submit" disabled={pending}>{pending ? "正在执行" : operation === "recall" ? "Run Recall" : "Run Explain"}<Radio aria-hidden="true" size={16} /></Button>
    </form>
  );
}

function ResultView({ result }: { result: MemoryEnvelope }) {
  const packet = result.response.response === "recall" ? result.response.payload as MemoryPacketData : null;
  return (
    <section className="lab-result">
      <div className="result-meta"><span>Request ID</span><code>{result.request_id}</code><span>Response</span><strong>{result.response.response}</strong></div>
      {packet ? <MemoryPacket packet={packet} /> : null}
      <JsonViewer value={result} />
    </section>
  );
}
