import { useQuery } from "@tanstack/react-query";
import { Activity, Database, Fingerprint, RadioTower } from "lucide-react";

import { StatusPill } from "../components/status-pill.js";
import { api } from "../lib/api.js";
import type { InstanceSummary, ScopeSelector } from "../lib/contracts.js";

const emptyScope: ScopeSelector = { tenantId: null, userId: null, agentId: null, projectId: null };

interface StatsEnvelope {
  request_id: string;
  response: {
    payload: { fragment_count: number; event_count: number };
  };
}

export function OverviewPage({ instance }: { instance: InstanceSummary }) {
  const status = useQuery({
    queryKey: ["instance-status", instance.id],
    queryFn: () => api.instanceStatus(instance.id),
    refetchInterval: 15_000,
  });
  const stats = useQuery({
    queryKey: ["instance-stats", instance.id],
    queryFn: () => api.memory<StatsEnvelope>(instance.id, "stats", { scope: emptyScope }),
    refetchInterval: 15_000,
  });

  return (
    <main className="console-page">
      <header className="page-header">
        <div><p className="page-kicker">OPERATOR OVERVIEW</p><h1>运行总览</h1></div>
        <p>{instance.name} · 只展示当前服务契约返回的数据。</p>
      </header>
      <div className="status-row">
        <StatusPill state={status.data?.health.status === "ok" ? "ok" : status.isPending ? "pending" : "warning"}>Health</StatusPill>
        <StatusPill state={status.data?.ready.status === "ready" ? "ok" : status.isPending ? "pending" : "warning"}>Ready</StatusPill>
        <StatusPill state={status.data?.serverReady ? "ok" : status.isPending ? "pending" : "warning"}>server_ready</StatusPill>
      </div>
      {status.isError || stats.isError ? <div className="error-band">无法读取实例状态。请检查地址、API Key 与 Namespace。</div> : null}
      <section className="metric-grid" aria-label="权威记录统计">
        <Metric icon={<Database />} label="Fragments" value={stats.data?.response.payload.fragment_count ?? "—"} />
        <Metric icon={<Activity />} label="Events" value={stats.data?.response.payload.event_count ?? "—"} />
        <Metric icon={<RadioTower />} label="Protocol" value={instance.protocolVersion} />
      </section>
      <section className="instance-facts">
        <h2>连接事实</h2>
        <Fact label="Namespace" value={instance.namespaceId} icon={<Fingerprint />} />
        <Fact label="Server URL" value={instance.baseUrl} />
        <Fact label="Last Request ID" value={stats.data?.request_id ?? "当前调用链未提供"} />
      </section>
    </main>
  );
}

function Metric({ icon, label, value }: { icon: React.ReactNode; label: string; value: string | number }) {
  return <div className="metric"><span>{icon}{label}</span><strong>{value}</strong></div>;
}
function Fact({ label, value, icon }: { label: string; value: string; icon?: React.ReactNode }) {
  return <div className="fact-row"><span>{icon}{label}</span><code title={value}>{value}</code></div>;
}
