import {
  ArrowRight,
  BadgeCheck,
  Binary,
  Boxes,
  Braces,
  Check,
  Cloud,
  Database,
  Fingerprint,
  GitBranch,
  LockKeyhole,
  Minus,
  Network,
  Radar,
  Route,
  Server,
  ShieldCheck,
  Workflow,
} from "lucide-react";
import { Link } from "react-router-dom";

import { MemoryUniverse } from "../components/memory-universe.js";

const liveCapabilities = ["Observe", "Recall", "Explain", "Runtime Start"];

const systemLayers = [
  {
    index: "01",
    title: "Authority",
    status: "IMMUTABLE",
    icon: Fingerprint,
    body: "Fragment、Event 与 EvidenceRef 保存发生过什么，是所有派生结构的唯一权威来源。",
  },
  {
    index: "02",
    title: "Cognitive Graph",
    status: "REBUILDABLE",
    icon: Network,
    body: "Point、Edge 与 Surface 表达状态、因果和主题，但始终保留来源，可随算法升级重建。",
  },
  {
    index: "03",
    title: "Bounded Recall",
    status: "TRACEABLE",
    icon: Radar,
    body: "十种 Recall 模式在硬预算、时间有效性和 Scope 权限内激活证据，并返回 Explain Trace。",
  },
  {
    index: "04",
    title: "Agent Runtime",
    status: "RECOVERABLE",
    icon: Workflow,
    body: "Run、Transition 与 Checkpoint 记录推演和行动；模型候选不会绕过验证成为事实。",
  },
] as const;

const runtimeStages = [
  { name: "Observe", status: "LIVE", detail: "Observation" },
  { name: "Recall", status: "LIVE", detail: "Bounded query" },
  { name: "Memory Packet", status: "LIVE", detail: "Model-independent" },
  { name: "Model", status: "LIVE", detail: "Provider" },
  { name: "Policy", status: "CORE", detail: "Risk boundary" },
  { name: "Approval", status: "CORE", detail: "Human gate" },
  { name: "Tool", status: "CORE", detail: "Controlled execute" },
  { name: "Commit", status: "LIVE", detail: "Unverified result" },
] as const;

const recallModes = [
  "Exact",
  "Semantic",
  "Temporal",
  "Causal",
  "Entity",
  "Associative",
  "Procedural",
  "Current State",
  "Historical State",
  "Deep Reconstruction",
];

const productSurfaces = [
  { icon: Database, title: "SQLite WAL / FTS5", body: "权威记录、版本化事件与真实 FTS5 执行路径。" },
  { icon: Network, title: "Point / Edge / Surface", body: "有来源的认知图、时间关系和版本化认知子空间。" },
  { icon: Braces, title: "Memory Packet", body: "模型无关的结构化上下文、证据、冲突与覆盖信息。" },
  { icon: Route, title: "Runtime Transitions", body: "Scope、Step、Deadline、Trace 与 EvidenceRef 持久化。" },
  { icon: ShieldCheck, title: "Governance", body: "ACL、Quarantine、Retention、Snapshot 与恢复边界。" },
  { icon: Binary, title: "Embedded / Sidecar / Server / MCP", body: "不同接入面复用同一个 Rust Core 与 CommandBus。" },
] as const;

const runtimeFacts = [
  { label: "RUN STATE", title: "每次转换都有版本", body: "Run、Step、Version 与 Control Checkpoint 写入 SQLite，非法状态跳转会被拒绝。" },
  { label: "RECOVERY", title: "恢复是一条显式路径", body: "恢复校验 Scope、Transition 连续性和版本，非幂等工具不会自动重放。" },
  { label: "FACT BOUNDARY", title: "结果不自动成为事实", body: "ToolResult 可以证伪 Prediction，但模型输出不能直接升级为 Verified Fact。" },
] as const;

const trustPrinciples = [
  { icon: Fingerprint, title: "权威记录", body: "原始 Fragment、Event 与 EvidenceRef 不被摘要、Embedding 或界面覆盖。" },
  { icon: LockKeyhole, title: "精确 Scope", body: "浏览器不提交 Grant，服务端授权发生在查询前与返回前。" },
  { icon: GitBranch, title: "证据链", body: "Transition 保存真实 EvidenceRef，不合成不存在的 VerificationMethod。" },
  { icon: BadgeCheck, title: "事实边界", body: "Prediction、Inference 与 Plan 不直接成为 Verified Fact。" },
] as const;

const plans = [
  {
    icon: Server,
    name: "Self-hosted",
    status: "LIVE",
    summary: "在自己的边界内运行 Server、Sidecar 与 MCP。",
    features: ["REST v1 与 gRPC", "精确 Scope 与 ACL", "SQLite WAL + BlobStore"],
    action: "开始连接",
    href: "/login",
    featured: true,
  },
  {
    icon: Cloud,
    name: "Managed Cloud",
    status: "PLATFORM",
    summary: "托管控制面与隔离 Sandbox 的商业方向。",
    features: ["尚未开放购买", "不展示虚构配额", "交付状态以能力矩阵为准"],
    featured: false,
  },
  {
    icon: ShieldCheck,
    name: "Enterprise",
    status: "ROADMAP",
    summary: "面向私有部署、治理与组织级接入。",
    features: ["私有网络部署方向", "治理与审计路线图", "价格与 SLA 尚未定义"],
    featured: false,
  },
] as const;

export function LandingPage() {
  return (
    <main className="min-h-screen bg-[#080b0a] text-white selection:bg-emerald-300 selection:text-black">
      <section className="relative min-h-[min(820px,calc(100svh-28px))] overflow-hidden border-b border-white/10" id="product">
        <MemoryUniverse />
        <div className="absolute inset-0 bg-black/36 md:bg-black/16" aria-hidden="true" />
        <div className="absolute inset-x-0 top-0 h-px bg-emerald-300/60" aria-hidden="true" />

        <PublicHeader />

        <div className="relative mx-auto flex min-h-[min(820px,calc(100svh-28px))] w-[min(1240px,calc(100%-40px))] flex-col justify-center pb-32 pt-32">
          <div className="max-w-3xl">
            <p className="mb-5 flex items-center gap-2 text-xs font-semibold text-emerald-300">
              <span className="size-1.5 bg-emerald-300" /> MEMORY INFRASTRUCTURE / AGENT RUNTIME
            </p>
            <h1 className="m-0 text-[clamp(58px,8vw,110px)] font-semibold leading-[0.92]">疆诚记忆</h1>
            <p className="mt-7 max-w-3xl text-xl leading-8 text-white/72 md:text-2xl">
              让 Agent 保留过去，并对每次行动负责。
            </p>
            <p className="mt-4 max-w-2xl text-sm leading-7 text-white/52 md:text-base">
              独立于模型的认知记忆系统与 Agent Runtime。原始证据始终可追溯，预测不会冒充事实。
            </p>
            <div className="mt-9 flex flex-wrap gap-3">
              <Link className="inline-flex h-12 items-center gap-2 bg-emerald-300 px-5 text-sm font-bold text-black hover:bg-emerald-200" to="/login">
                进入控制台 <ArrowRight size={17} aria-hidden="true" />
              </Link>
              <a className="inline-flex h-12 items-center gap-2 border border-white/18 bg-black/35 px-5 text-sm font-semibold text-white hover:bg-white/10" href="#runtime">
                查看完整链路 <GitBranch size={16} aria-hidden="true" />
              </a>
            </div>
          </div>

          <div className="absolute bottom-0 left-0 grid w-full grid-cols-2 border-x border-t border-white/12 bg-black/62 backdrop-blur-sm sm:grid-cols-4 md:w-[720px]" aria-label="当前在线能力">
            {liveCapabilities.map((capability) => (
              <div className="border-b border-r border-white/10 p-4 last:border-r-0 sm:border-b-0 md:p-5" key={capability}>
                <span className="text-[10px] font-bold text-emerald-300">LIVE</span>
                <strong className="mt-2 block text-sm">{capability}</strong>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="bg-[#f5f7f5] py-28 text-[#111513] md:py-36" id="architecture">
        <div className="mx-auto w-[min(1180px,calc(100%-40px))]">
          <header className="mx-auto max-w-4xl text-center">
            <p className="public-kicker text-emerald-700">MEMORY-NATIVE ARCHITECTURE</p>
            <h2 className="mt-5 text-4xl font-semibold leading-[1.08] md:text-7xl">从原始证据，到可恢复的行动闭环。</h2>
            <p className="mx-auto mt-7 max-w-2xl text-sm leading-7 text-black/52 md:text-base md:leading-8">不是把聊天记录塞进向量库，而是让事实、认知、召回和行动在同一条可审计链路中各守边界。</p>
          </header>

          <div aria-label="记忆层级" className="mt-20 border-y border-black/12">
            {systemLayers.map((layer) => (
              <article className="grid gap-6 border-b border-black/10 py-8 last:border-b-0 md:grid-cols-[72px_1fr_160px_1.35fr] md:items-center md:py-10" key={layer.title}>
                <span className="flex items-center gap-3 font-mono text-[10px] text-black/35"><layer.icon className="text-emerald-700" size={19} aria-hidden="true" />{layer.index}</span>
                <h3 className="m-0 text-2xl font-semibold md:text-3xl">{layer.title}</h3>
                <strong className="text-[9px] text-emerald-700">{layer.status}</strong>
                <p className="m-0 text-sm leading-7 text-black/52">{layer.body}</p>
              </article>
            ))}
          </div>
        </div>
      </section>

      <section className="border-y border-white/10 bg-[#080b0a] py-28 md:py-36" id="runtime">
        <div className="mx-auto w-[min(1240px,calc(100%-40px))]">
          <header className="mx-auto max-w-4xl text-center">
            <p className="public-kicker text-emerald-300">ONE TRUSTED LOOP</p>
            <h2 className="mt-5 text-4xl font-semibold leading-[1.08] md:text-7xl">记忆不是附属插件，而是每一步行动的上下文与证据。</h2>
            <p className="mx-auto mt-7 max-w-2xl text-sm leading-7 text-white/48">当前 Web 已接通 FinalResponse 路径；Policy、Approval 与 Tool 仍按真实远程契约状态标为 CORE。</p>
          </header>

          <div className="mt-20 overflow-hidden rounded-lg border border-white/12 bg-[#0f1412]" aria-label="Agent Runtime 完整链路">
            <div className="overflow-x-auto px-7 py-10 md:px-10">
              <div className="relative grid min-w-[1040px] grid-cols-8 gap-8 before:absolute before:left-6 before:right-6 before:top-[21px] before:h-px before:bg-white/12">
                {runtimeStages.map((stage, index) => (
                  <div className="relative" key={stage.name}>
                    <span className={`relative z-10 grid size-11 place-items-center rounded-full border bg-[#0f1412] font-mono text-[10px] ${stage.status === "LIVE" ? "border-emerald-300 text-emerald-300" : "border-sky-300 text-sky-300"}`}>{String(index + 1).padStart(2, "0")}</span>
                    <strong className="mt-6 block text-sm">{stage.name}</strong>
                    <small className="mt-2 block text-[9px] text-white/38">{stage.status} / {stage.detail}</small>
                  </div>
                ))}
              </div>
            </div>
            <div className="grid border-t border-white/10 lg:grid-cols-3">
              {runtimeFacts.map((fact) => (
                <article className="border-b border-white/10 p-7 last:border-b-0 lg:border-b-0 lg:border-r lg:last:border-r-0" key={fact.label}>
                  <span className="text-[9px] font-bold text-emerald-300">{fact.label}</span>
                  <h3 className="mt-5 text-xl font-semibold">{fact.title}</h3>
                  <p className="mt-3 text-xs leading-6 text-white/45">{fact.body}</p>
                </article>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="bg-[#eaf0f3] py-28 text-[#111513] md:py-36" id="recall">
        <div className="mx-auto w-[min(1180px,calc(100%-40px))]">
          <header className="mx-auto max-w-4xl text-center">
            <p className="public-kicker text-sky-700">BOUNDED PATTERN COMPLETION</p>
            <h2 className="mt-5 text-4xl font-semibold leading-[1.08] md:text-7xl">十种 Recall 模式，同一份证据边界。</h2>
            <p className="mx-auto mt-7 max-w-2xl text-sm leading-7 text-black/52">种子、邻居、跳数、节点、时间和 Scope 都有硬限制；结果通过 Memory Packet 与 Recall Trace 交付。</p>
          </header>

          <MemoryPacketPreview />

          <div className="mt-10 flex flex-wrap justify-center gap-x-6 gap-y-3 text-[10px] font-semibold text-black/46" aria-label="Recall 模式">
            {recallModes.map((mode, index) => <span key={mode}><b className="mr-2 font-mono text-emerald-700">{String(index + 1).padStart(2, "0")}</b>{mode}</span>)}
          </div>
        </div>
      </section>

      <section className="bg-white py-28 text-[#111513] md:py-36">
        <div className="mx-auto w-[min(1180px,calc(100%-40px))]">
          <header className="mx-auto max-w-4xl text-center">
            <p className="public-kicker text-emerald-700">ONE CORE, MULTIPLE SURFACES</p>
            <h2 className="mt-5 text-4xl font-semibold leading-[1.08] md:text-7xl">既是记忆工具，也是一条完整链路。</h2>
            <p className="mx-auto mt-7 max-w-2xl text-sm leading-7 text-black/52">每个入口都回到同一个 CommandBus、Scope、ACL 与权威存储，Web 层不复制业务语义。</p>
          </header>

          <div className="mt-20 border-y border-black/12">
            {productSurfaces.map((surface, index) => (
              <article className="grid gap-5 border-b border-black/10 py-7 last:border-b-0 md:grid-cols-[64px_1fr_1.4fr] md:items-center" key={surface.title}>
                <span className="flex items-center gap-3 font-mono text-[9px] text-black/30"><surface.icon className="text-emerald-700" size={18} aria-hidden="true" />{String(index + 1).padStart(2, "0")}</span>
                <h3 className="m-0 text-xl font-semibold">{surface.title}</h3>
                <p className="m-0 text-sm leading-7 text-black/50">{surface.body}</p>
              </article>
            ))}
          </div>

          <p className="m-0 overflow-hidden border-b border-black/12 py-12 text-center text-[clamp(32px,6vw,76px)] font-semibold text-black/12">EMBEDDED · SIDECAR · SERVER · MCP</p>
        </div>
      </section>

      <section className="bg-[#eef3f7] py-28 text-[#111513] md:py-36" id="security">
        <div className="mx-auto grid w-[min(1180px,calc(100%-40px))] gap-16 lg:grid-cols-[0.9fr_1.1fr]">
          <header>
            <p className="public-kicker text-sky-700">TRUST BOUNDARY</p>
            <h2 className="mt-5 max-w-xl text-4xl font-semibold leading-[1.08] md:text-6xl">证据先于推断，权限先于召回。</h2>
            <p className="mt-7 max-w-md text-sm leading-7 text-black/52">原始记录永远高于摘要、Embedding、图关系和界面展示；模型候选只有获得后续证据，才可能进入事实链。</p>
          </header>
          <div className="border-t border-black/12">
            {trustPrinciples.map((principle) => (
              <article className="grid gap-4 border-b border-black/12 py-7 sm:grid-cols-[44px_140px_1fr] sm:items-start" key={principle.title}>
                <span className="grid size-9 place-items-center rounded-lg bg-white text-sky-800"><principle.icon size={17} aria-hidden="true" /></span>
                <h3 className="m-0 text-base font-semibold">{principle.title}</h3>
                <p className="m-0 text-sm leading-7 text-black/50">{principle.body}</p>
              </article>
            ))}
          </div>
        </div>
      </section>

      <section className="border-y border-black/8 bg-[#f7f8f7] py-28 text-[#111513] md:py-36" id="deployment">
        <div className="mx-auto w-[min(1180px,calc(100%-40px))]">
          <header className="mx-auto max-w-3xl text-center">
            <p className="public-kicker text-emerald-700">DEPLOYMENT</p>
            <h2 className="mt-5 text-4xl font-semibold leading-[1.08] md:text-7xl">从本地工具到完整链路。</h2>
            <p className="mx-auto mt-7 max-w-2xl text-sm leading-7 text-black/52">同一核心语义服务于 Embedded、Sidecar、Server 与 MCP；平台能力按交付状态逐步开放。</p>
          </header>
          <div className="mt-16 grid gap-4 lg:grid-cols-3" id="pricing">
            {plans.map((plan) => (
              <article className={`flex min-h-[410px] flex-col rounded-lg border p-7 ${plan.featured ? "border-[#111513] bg-[#111513] text-white" : "border-black/10 bg-white"}`} key={plan.name}>
                <div className="flex items-center justify-between">
                  <span className={`grid size-10 place-items-center rounded-lg ${plan.featured ? "bg-white/8 text-emerald-300" : "bg-[#eef3ef] text-emerald-800"}`}><plan.icon size={19} aria-hidden="true" /></span>
                  <span className={`text-[9px] font-bold ${plan.status === "LIVE" ? "text-emerald-500" : "text-amber-600"}`}>{plan.status}</span>
                </div>
                <h3 className="mt-9 text-2xl font-semibold">{plan.name}</h3>
                <p className={`mt-3 min-h-14 text-sm leading-6 ${plan.featured ? "text-white/55" : "text-black/50"}`}>{plan.summary}</p>
                <ul className={`mt-8 grid list-none gap-4 border-t pt-7 text-sm ${plan.featured ? "border-white/10 text-white/70" : "border-black/8 text-black/60"}`}>
                  {plan.features.map((feature) => (
                    <li className="flex items-start gap-3" key={feature}>{plan.status === "LIVE" ? <Check className="mt-0.5 text-emerald-500" size={15} aria-hidden="true" /> : <Minus className="mt-0.5 text-amber-600" size={15} aria-hidden="true" />}{feature}</li>
                  ))}
                </ul>
                {"href" in plan ? <Link className="mt-auto inline-flex h-11 w-full items-center justify-center gap-2 rounded-lg bg-emerald-300 text-sm font-bold text-black hover:bg-emerald-200" to={plan.href}>{plan.action} <ArrowRight size={16} aria-hidden="true" /></Link> : null}
              </article>
            ))}
          </div>
        </div>
      </section>

      <footer className="bg-[#080b0a] py-8 text-white/45">
        <div className="mx-auto flex w-[min(1180px,calc(100%-40px))] flex-wrap items-center justify-between gap-4 text-xs">
          <Brand compact />
          <span>Fragment / Event / EvidenceRef remain authoritative.</span>
        </div>
      </footer>
    </main>
  );
}

function PublicHeader() {
  return (
    <header className="absolute inset-x-0 top-0 z-20">
      <div className="mx-auto mt-5 flex h-16 w-[min(1180px,calc(100%-32px))] items-center gap-6 border border-white/12 bg-black/70 px-4 backdrop-blur-md md:px-5">
        <Brand />
        <nav aria-label="公开导航" className="ml-auto hidden items-center gap-6 text-xs text-white/62 lg:flex">
          <a href="#product">产品</a>
          <a href="#runtime">Runtime</a>
          <a href="#security">安全治理</a>
          <a href="#deployment">部署</a>
          <a href="#pricing">定价</a>
        </nav>
        <div className="ml-auto flex items-center gap-2 lg:ml-3">
          <Link className="hidden h-9 items-center px-3 text-xs font-semibold text-white/70 hover:text-white sm:inline-flex" to="/login">登录</Link>
          <Link className="inline-flex h-9 items-center gap-2 bg-emerald-300 px-4 text-xs font-bold text-black hover:bg-emerald-200" to="/login">Get Started <ArrowRight size={14} aria-hidden="true" /></Link>
        </div>
      </div>
    </header>
  );
}

function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <div className="flex shrink-0 items-center gap-2.5">
      <span className={`${compact ? "size-7" : "size-8"} grid place-items-center bg-emerald-300 text-black`}><Boxes size={compact ? 15 : 17} strokeWidth={2.5} aria-hidden="true" /></span>
      <span className="grid leading-none"><strong className="text-sm">疆诚记忆</strong>{compact ? null : <small className="mt-1 text-[8px] font-semibold text-white/42">JIANGCHENG MEMORY</small>}</span>
    </div>
  );
}

function MemoryPacketPreview() {
  return (
    <div aria-label="Memory Packet 工作区预览" className="mt-20 overflow-hidden rounded-lg border border-black/12 bg-white shadow-[0_28px_80px_rgba(18,38,48,0.12)]">
      <header className="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-black/10 px-5 py-3">
        <div className="flex items-center gap-3">
          <span className="grid size-8 place-items-center rounded-lg bg-[#111513] text-emerald-300"><Braces size={16} aria-hidden="true" /></span>
          <span className="grid gap-1"><strong className="text-xs">Recall Lab</strong><small className="text-[8px] font-bold text-black/35">LIVE CONTRACT / MEMORY PACKET</small></span>
        </div>
        <span className="flex items-center gap-2 text-[9px] font-semibold text-emerald-800"><i className="size-1.5 rounded-full bg-emerald-500" />SERVER AUTHORIZED</span>
      </header>

      <div className="grid lg:grid-cols-[340px_1fr]">
        <aside className="bg-[#111513] p-6 text-white md:p-8">
          <p className="m-0 text-[9px] font-bold text-emerald-300">RECALL QUERY</p>
          <div className="mt-6 border-b border-white/12 pb-5">
            <span className="text-[9px] text-white/35">query_text</span>
            <code className="mt-2 block text-sm text-white/80">&quot;request context&quot;</code>
          </div>
          <div className="grid grid-cols-2 gap-x-5 gap-y-6 py-6 text-[9px]">
            <span className="grid gap-2 text-white/35">MODE<strong className="text-xs font-medium text-white/75">Current State</strong></span>
            <span className="grid gap-2 text-white/35">SCOPE<strong className="text-xs font-medium text-white/75">Authorized</strong></span>
            <span className="grid gap-2 text-white/35">PROPAGATION<strong className="text-xs font-medium text-white/75">Hard bounded</strong></span>
            <span className="grid gap-2 text-white/35">EFFECTIVE TIME<strong className="text-xs font-medium text-white/75">Request time</strong></span>
          </div>
          <span className="mt-3 inline-flex h-10 w-full items-center justify-center rounded-lg bg-emerald-300 text-xs font-bold text-black">Run Recall</span>
        </aside>

        <div className="min-w-0 p-6 md:p-8">
          <div className="flex flex-wrap items-start justify-between gap-4 border-b border-black/10 pb-6">
            <div><p className="m-0 text-[9px] font-bold text-sky-700">OUTPUT CONTRACT</p><h3 className="mt-2 text-2xl font-semibold">Memory Packet</h3></div>
            <span className="rounded-full bg-emerald-50 px-3 py-2 text-[9px] font-bold text-emerald-800">TRACEABLE</span>
          </div>

          <div className="grid md:grid-cols-2">
            {[
              ["Verified Fragments", "Evidence-linked authority records"],
              ["Current State", "Effective-time filtered"],
              ["Missing Evidence", "Gaps remain explicit"],
              ["Coverage", "Bounds and truncation reported"],
            ].map(([title, body], index) => (
              <section className={`min-h-32 py-6 ${index % 2 === 0 ? "md:border-r md:pr-6" : "md:pl-6"} ${index < 2 ? "border-b border-black/10" : ""}`} key={title}>
                <span className="font-mono text-[9px] text-black/30">{String(index + 1).padStart(2, "0")}</span>
                <h4 className="mt-4 text-sm font-semibold">{title}</h4>
                <p className="mt-2 text-[11px] leading-5 text-black/45">{body}</p>
              </section>
            ))}
          </div>

          <div className="flex flex-wrap items-center justify-between gap-3 border-t border-black/10 pt-5">
            <span className="flex items-center gap-2 text-[10px] font-semibold"><Route className="text-sky-700" size={14} aria-hidden="true" />Recall Trace</span>
            <code className="text-[9px] text-black/38">seed -&gt; bounded propagation -&gt; ACL-filtered return</code>
          </div>
        </div>
      </div>
    </div>
  );
}
