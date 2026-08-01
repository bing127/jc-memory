import { Activity, Database, Fingerprint, RadioTower } from "lucide-react";

import { StatusPill } from "../components/status-pill.js";
import type { InstanceSummary, OverviewSnapshot } from "../lib/contracts.js";
import { useInstanceStreamSubscription, useInstanceStreamTopic } from "../lib/instance-stream.js";

export function OverviewPage({ instance }: { instance: InstanceSummary }) {
  useInstanceStreamSubscription({ topic: "overview" });
  const overview = useInstanceStreamTopic<OverviewSnapshot>("overview");
  const status = overview.data?.status;
  const stats = overview.data?.stats;
  const pending = !overview.data && !overview.error;

  return (
    <main className="console-page">
      <header className="page-header">
        <div><p className="page-kicker">OPERATOR OVERVIEW</p><h1>运行总览</h1></div>
        <p>{instance.name} · 只展示当前服务契约返回的数据。</p>
      </header>
      <div className="status-row">
        <StatusPill state={status?.health.status === "ok" ? "ok" : pending ? "pending" : "warning"}>Health</StatusPill>
        <StatusPill state={status?.ready.status === "ready" ? "ok" : pending ? "pending" : "warning"}>Ready</StatusPill>
        <StatusPill state={status?.serverReady ? "ok" : pending ? "pending" : "warning"}>server_ready</StatusPill>
      </div>
      <div
        className={`error-band ${overview.error ? "visible" : "invisible"}`}
        role="alert"
        aria-hidden={!overview.error}
      >
        {overview.error ? "无法刷新实例状态。请检查地址、API Key 与 Namespace。" : "总览频道正常"}
      </div>
      <section className="metric-grid" aria-label="权威记录统计">
        <Metric icon={<Database />} label="Fragments" value={stats?.response.payload.fragment_count ?? "—"} />
        <Metric icon={<Activity />} label="Events" value={stats?.response.payload.event_count ?? "—"} />
        <Metric icon={<RadioTower />} label="Protocol" value={instance.protocolVersion} />
      </section>
      <section className="instance-facts">
        <h2>连接事实</h2>
        <Fact label="Namespace" value={instance.namespaceId} icon={<Fingerprint />} />
        <Fact label="Server URL" value={instance.baseUrl} />
        <Fact label="Last Request ID" value={stats?.request_id ?? "当前调用链未提供"} />
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
