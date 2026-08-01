type CapabilityStatus = "LIVE" | "CORE" | "ROADMAP" | "PLATFORM";

export interface CapabilityRecord {
  name: string;
  status: CapabilityStatus;
  surface: string;
  productionNavigation: boolean;
  evidence: string;
  boundary: string;
}

export const capabilityRegistry: CapabilityRecord[] = [
  {
    name: "Service Health / Ready",
    status: "LIVE",
    surface: "Overview",
    productionNavigation: true,
    evidence: "services/memory-server/src/lib.rs",
    boundary: "只返回 status=ok、status=ready 与 server_ready。",
  },
  {
    name: "Scope Stats",
    status: "LIVE",
    surface: "Overview",
    productionNavigation: true,
    evidence: "crates/memory-protocol/src/command.rs",
    boundary: "只统计精确 Scope 的 Fragment 与 Event。",
  },
  {
    name: "Observe",
    status: "LIVE",
    surface: "Memory Lab",
    productionNavigation: true,
    evidence: "services/memory-server/src/lib.rs",
    boundary: "原始 Fragment 进入共享 CommandBus；写失败不自动重试。",
  },
  {
    name: "Recall / Explain",
    status: "LIVE",
    surface: "Memory Lab",
    productionNavigation: true,
    evidence: "crates/memory-protocol/src/recall.rs",
    boundary: "十种 Recall Mode、硬预算、Memory Packet 与 Explain Trace。",
  },
  {
    name: "Commit / Correct / Forget",
    status: "LIVE",
    surface: "Memory Lab",
    productionNavigation: true,
    evidence: "services/memory-server/src/lib.rs",
    boundary: "Commit 不自动重试；Correct/Forget 追加审计记录，Forget 不执行 Hard Delete。",
  },
  {
    name: "Database / Blob Health",
    status: "CORE",
    surface: "Capability matrix",
    productionNavigation: false,
    evidence: "docs/superpowers/specs/2026-07-31-jiangcheng-memory-web-control-plane-design.md",
    boundary: "MemoryEngine 能力存在，当前 Server 没有受保护的查询契约。",
  },
  {
    name: "Runtime Run / Transition / Checkpoint",
    status: "LIVE",
    surface: "Agent Runtime",
    productionNavigation: true,
    evidence: "services/memory-server/src/lib.rs",
    boundary: "只读、精确 Scope 查询；当前不提供启动、审批、Tool 执行或模型配置。",
  },
  {
    name: "Runtime Start (FinalResponse)",
    status: "LIVE",
    surface: "Agent Runtime",
    productionNavigation: true,
    evidence: "services/memory-server/src/runtime_backend.rs",
    boundary: "依赖 Server 配置的 Provider；模型响应以 Inference + Unverified 追加，不写为 Verified Fact。",
  },
  {
    name: "Runtime Approval / Tool / Control / Provider UI",
    status: "CORE",
    surface: "Capability matrix",
    productionNavigation: false,
    evidence: "docs/runtime.zh-CN.md",
    boundary: "审批续跑、Tool Execute、Pause/Resume 与 Provider 管理尚未形成完整持久化远程契约。",
  },
  {
    name: "Governance / Recovery Console",
    status: "ROADMAP",
    surface: "Capability matrix",
    productionNavigation: false,
    evidence: "docs/recovery.zh-CN.md",
    boundary: "需要分页、Scope、Operation 授权与远程契约后才能进入导航。",
  },
  {
    name: "Cross-protocol Production E2E",
    status: "ROADMAP",
    surface: "Capability matrix",
    productionNavigation: false,
    evidence: "docs/superpowers/plans/2026-07-29-jiangcheng-memory-roadmap.md",
    boundary: "Fixture 契约不能描述为外部平台或生产环境实机验证。",
  },
  {
    name: "Managed Sandbox",
    status: "PLATFORM",
    surface: "Get Started",
    productionNavigation: false,
    evidence: "docs/superpowers/specs/2026-07-31-jiangcheng-memory-web-control-plane-design.md",
    boundary: "Provision、计费、配额和生命周期服务尚未实现。",
  },
];

export function CapabilitiesPage() {
  return (
    <main className="console-page">
      <header className="page-header">
        <div><p className="page-kicker">CAPABILITY REGISTRY</p><h1>能力状态矩阵</h1></div>
        <p>官网、控制台导航和后台操作使用同一状态边界。</p>
      </header>
      <div className="capability-legend">
        <Status status="LIVE" /><span>当前远程契约可调用</span>
        <Status status="CORE" /><span>Rust 能力存在，缺少 Web 适配</span>
        <Status status="ROADMAP" /><span>文档已定义，尚未完成</span>
        <Status status="PLATFORM" /><span>控制面平台基础设施</span>
      </div>
      <div className="capability-table-wrap">
        <table className="capability-table">
          <thead><tr><th>能力</th><th>状态</th><th>Surface</th><th>证据</th><th>当前边界</th></tr></thead>
          <tbody>
            {capabilityRegistry.map((capability) => (
              <tr key={capability.name}>
                <td><strong>{capability.name}</strong>{capability.productionNavigation ? <small>PRODUCTION NAV</small> : null}</td>
                <td><Status status={capability.status} /></td>
                <td>{capability.surface}</td>
                <td><code>{capability.evidence}</code></td>
                <td>{capability.boundary}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </main>
  );
}

function Status({ status }: { status: CapabilityStatus }) {
  return <span className={`capability-status capability-${status.toLowerCase()}`}>{status}</span>;
}
