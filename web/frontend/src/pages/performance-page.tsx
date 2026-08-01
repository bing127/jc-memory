import { useEffect, useState } from "react";
import { AlertTriangle, BarChart3, FileJson, Gauge, Radio, Upload } from "lucide-react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import type {
  InstanceSummary,
  PerformanceOperationName,
  PerformanceSnapshot,
} from "../lib/contracts.js";
import {
  appendPerformanceHistory,
  deriveOperationSeries,
  formatMetric,
  liveTimelineDomain,
  parseLoadTestReport,
  reportChartData,
  summarizeLoadTestReport,
  type LoadTestReport,
  type PerformanceHistoryEntry,
} from "../lib/performance.js";

export function PerformancePage({ instance }: { instance: InstanceSummary }) {
  const [report, setReport] = useState<LoadTestReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"static" | "live">("static");
  const [operation, setOperation] = useState<PerformanceOperationName>("recall");
  const [history, setHistory] = useState<PerformanceHistoryEntry[]>([]);
  const summary = report ? summarizeLoadTestReport(report) : null;
  const chartData = report ? reportChartData(report) : null;
  const [streamState, setStreamState] = useState<"idle" | "connecting" | "connected" | "error">("idle");

  useEffect(() => {
    if (tab !== "live") {
      setStreamState("idle");
      return;
    }
    let disposed = false;
    let reconnectAttempt = 0;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let socket: WebSocket | null = null;

    const connect = () => {
      if (disposed) return;
      setStreamState("connecting");
      socket = new WebSocket(metricsStreamUrl(instance.id));
      socket.addEventListener("open", () => {
        reconnectAttempt = 0;
        setStreamState("connected");
      });
      socket.addEventListener("message", (event) => {
        try {
          const message = JSON.parse(String(event.data)) as MetricsStreamMessage;
          if (message.type === "snapshot") {
            setStreamState("connected");
            setHistory((current) => appendPerformanceHistory(current, message.snapshot));
          } else if (message.type === "gap") {
            setStreamState("error");
            setHistory((current) => appendPerformanceHistory(current, null, message.sampledAt));
          }
        } catch {
          setStreamState("error");
        }
      });
      socket.addEventListener("error", () => setStreamState("error"));
      socket.addEventListener("close", () => {
        socket = null;
        if (disposed) return;
        setStreamState("error");
        const delay = Math.min(500 * (2 ** reconnectAttempt), 5_000);
        reconnectAttempt += 1;
        reconnectTimer = setTimeout(connect, delay);
      });
    };

    connect();
    return () => {
      disposed = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      socket?.close();
      socket = null;
    };
  }, [instance.id, tab]);

  async function handleFileChange(event: React.ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    try {
      const parsed = parseLoadTestReport(JSON.parse(await file.text()));
      setReport(parsed);
      setError(null);
    } catch (cause) {
      setReport(null);
      setError(cause instanceof Error ? cause.message : "无法读取压测报告");
    }
  }

  return (
    <main className="console-page performance-page">
      <header className="page-header">
        <div><p className="page-kicker">PERFORMANCE OBSERVABILITY</p><h1>性能观测</h1></div>
        <p>{instance.name} · 对比本地压测报告，并观察实例实时请求、延迟与背压。</p>
      </header>

      <div className="performance-tabs" role="tablist" aria-label="性能视图">
        <button type="button" role="tab" aria-selected={tab === "static"} className={tab === "static" ? "is-active" : ""} onClick={() => setTab("static")}>
          <FileJson aria-hidden="true" />静态报告
        </button>
        <button type="button" role="tab" aria-selected={tab === "live"} className={tab === "live" ? "is-active" : ""} onClick={() => setTab("live")}>
          <Gauge aria-hidden="true" />实时监控
        </button>
      </div>

      {tab === "static" ? (
        <>
          <section className="performance-import" aria-label="压测报告导入区">
            <div>
              <p className="section-kicker">LOCAL FILE</p>
              <h2>导入版本化压测报告</h2>
              <p>选择由 `scripts/benchmark/http-load.mjs` 生成的 JSON。文件只在当前浏览器中读取，不会上传。</p>
            </div>
            <label className="performance-upload">
              <Upload aria-hidden="true" />
              <span>选择 JSON 文件</span>
              <input aria-label="导入压测报告" type="file" accept="application/json,.json" onChange={handleFileChange} />
            </label>
          </section>

          {error ? <div className="error-band" role="alert"><AlertTriangle aria-hidden="true" />{error}</div> : null}
          {!report || !summary || !chartData ? (
            <div className="performance-empty"><BarChart3 aria-hidden="true" /><span>导入报告后查看延迟、吞吐和状态码图表。</span></div>
          ) : (
            <ReportView report={report} summary={summary} chartData={chartData} />
          )}
        </>
      ) : null}
      {tab === "live" ? (
        <LiveMonitor
          operation={operation}
          onOperationChange={setOperation}
          history={history}
          pending={streamState === "connecting"}
          failed={streamState === "error"}
          connected={streamState === "connected"}
        />
      ) : null}
    </main>
  );
}

type MetricsStreamMessage =
  | { type: "snapshot"; snapshot: PerformanceSnapshot }
  | { type: "gap"; sampledAt: string; message: string };

function metricsStreamUrl(instanceId: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/v1/instances/${instanceId}/metrics/stream`;
}

const operationLabels: Record<PerformanceOperationName, string> = {
  observe: "Observe",
  commit: "Commit",
  recall: "Recall",
  stats: "Stats",
  correct: "Correct",
  forget: "Forget",
  explain: "Explain",
  command: "Command",
};

function LiveMonitor({ operation, onOperationChange, history, pending, failed, connected }: {
  operation: PerformanceOperationName;
  onOperationChange(operation: PerformanceOperationName): void;
  history: PerformanceHistoryEntry[];
  pending: boolean;
  failed: boolean;
  connected: boolean;
}) {
  const series = deriveOperationSeries(history, operation);
  const timelineDomain = liveTimelineDomain(series);
  const latest = [...series].reverse().find((point) => !point.gap) ?? null;
  const lastSuccess = [...history].reverse().find((entry) => entry.snapshot !== null) ?? null;
  const gapCount = history.filter((entry) => entry.snapshot === null).length;

  return (
    <section className="live-monitor" aria-label="实时性能监控">
      <header className="live-toolbar">
        <div><p className="section-kicker">LIVE METRICS · WEBSOCKET</p><h2>实例实时性能</h2></div>
        <label>Operation
          <select value={operation} onChange={(event) => onOperationChange(event.target.value as PerformanceOperationName)}>
            {Object.entries(operationLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </label>
        <span className={`live-connection-state ${connected ? "is-connected" : ""}`}>
          <Radio aria-hidden="true" />{connected ? "实时流已连接" : pending ? "正在连接" : "等待重连"}
        </span>
      </header>

      {failed ? (
        <div className="error-band" role="alert">
          <AlertTriangle aria-hidden="true" />实时采样中断，图表已保留最后成功数据且缺口不会补零。
        </div>
      ) : null}
      {pending && !lastSuccess ? (
        <div className="performance-empty"><Radio aria-hidden="true" /><span>正在读取实例实时指标。</span></div>
      ) : (
        <>
          <section className="performance-metrics live-metrics" aria-label="实时核心指标">
            <Metric label="RPS" value={formatMetric(latest?.rps ?? null, "req/s")} />
            <Metric label="成功率" value={latest?.successRate === null || latest?.successRate === undefined ? "未采集" : `${latest.successRate.toFixed(1)}%`} />
            <Metric label="in-flight" value={latest?.inFlight?.toLocaleString("zh-CN") ?? "未采集"} />
            <Metric label="P50" value={formatMetric(latest?.p50 ?? null, "ms")} />
            <Metric label="P95" value={formatMetric(latest?.p95 ?? null, "ms")} />
            <Metric label="P99" value={formatMetric(latest?.p99 ?? null, "ms")} />
            <Metric label="429 背压" value={latest?.rateLimited?.toLocaleString("zh-CN") ?? "未采集"} />
            <Metric label="Timeout" value={latest?.timeouts?.toLocaleString("zh-CN") ?? "未采集"} />
          </section>

          <div className="performance-meta live-meta">
            <span>Operation <code>{operation}</code></span>
            <span aria-label={`样本 ${history.length} / 60`}>样本 <code>{history.length}</code> / 60</span>
            <span>采样缺口 {gapCount}</span>
            <span>最后成功采样 <code>{lastSuccess?.sampledAt ?? "尚无"}</code></span>
          </div>

          <section className="performance-chart-grid" aria-label="实时性能图表">
            <ChartPanel title="请求吞吐与 in-flight" description="实时流按时间从左向右推进；采样失败处断线">
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={series} margin={{ top: 12, right: 16, left: 0, bottom: 8 }}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#dce2de" />
                  <XAxis
                    dataKey="timestamp"
                    type="number"
                    scale="time"
                    domain={timelineDomain}
                    allowDataOverflow
                    tick={{ fontSize: 10 }}
                    tickFormatter={formatTimelineTick}
                  />
                  <YAxis yAxisId="left" tick={{ fontSize: 10 }} />
                  <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 10 }} />
                  <Tooltip labelFormatter={(value) => formatTimelineTick(Number(value))} />
                  <Legend wrapperStyle={{ fontSize: 10 }} />
                  <Line yAxisId="left" type="monotone" dataKey="rps" name="RPS" stroke="#2563eb" strokeWidth={2} dot={false} isAnimationActive={false} connectNulls={false} />
                  <Line yAxisId="right" type="monotone" dataKey="inFlight" name="in-flight" stroke="#087a45" strokeWidth={2} dot={false} isAnimationActive={false} connectNulls={false} />
                </LineChart>
              </ResponsiveContainer>
            </ChartPanel>
            <ChartPanel title="实时延迟分位数（毫秒）" description="当前累计 Histogram 分位数，最新样本在右侧">
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={series} margin={{ top: 12, right: 16, left: 0, bottom: 8 }}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#dce2de" />
                  <XAxis
                    dataKey="timestamp"
                    type="number"
                    scale="time"
                    domain={timelineDomain}
                    allowDataOverflow
                    tick={{ fontSize: 10 }}
                    tickFormatter={formatTimelineTick}
                  />
                  <YAxis tick={{ fontSize: 10 }} />
                  <Tooltip
                    labelFormatter={(value) => formatTimelineTick(Number(value))}
                    formatter={(value) => formatMetric(typeof value === "number" ? value : null, "ms")}
                  />
                  <Legend wrapperStyle={{ fontSize: 10 }} />
                  <Line type="monotone" dataKey="p50" name="P50" stroke="#22c979" strokeWidth={2} dot={false} isAnimationActive={false} connectNulls={false} />
                  <Line type="monotone" dataKey="p95" name="P95" stroke="#e3a008" strokeWidth={2} dot={false} isAnimationActive={false} connectNulls={false} />
                  <Line type="monotone" dataKey="p99" name="P99" stroke="#c2410c" strokeWidth={2} dot={false} isAnimationActive={false} connectNulls={false} />
                </LineChart>
              </ResponsiveContainer>
            </ChartPanel>
          </section>
        </>
      )}
    </section>
  );
}

function formatTimelineTick(timestamp: number): string {
  if (!Number.isFinite(timestamp)) return "";
  return new Date(timestamp).toLocaleTimeString("zh-CN", { hour12: false });
}

function ReportView({ report, summary, chartData }: {
  report: LoadTestReport;
  summary: ReturnType<typeof summarizeLoadTestReport>;
  chartData: ReturnType<typeof reportChartData>;
}) {
  return (
    <div className="performance-report">
      <section className="performance-metrics" aria-label="压测核心指标">
        <Metric label="总请求数" value={summary.requests.toLocaleString("zh-CN")} />
        <Metric label="成功率" value={summary.successRate === null ? "未采集" : `${summary.successRate.toFixed(1)}%`} />
        <Metric label="平均吞吐" value={formatMetric(summary.throughputRps, "req/s")} />
        <Metric label="最高 P50" value={formatMetric(summary.latencyMax.p50, "ms")} />
        <Metric label="最高 P95" value={formatMetric(summary.latencyMax.p95, "ms")} />
        <Metric label="最高 P99" value={formatMetric(summary.latencyMax.p99, "ms")} />
        <Metric label="429 背压" value={summary.status429.toLocaleString("zh-CN")} />
      </section>

      <div className="performance-meta">
        <span>目标 <code>{report.config.target}</code></span>
        <span>提交 <code>{report.provenance.git_commit}</code></span>
        <span>平台 <code>{report.provenance.platform}</code></span>
        <span>{report.provenance.dirty ? "工作树有未提交修改" : "工作树干净"}</span>
      </div>

      <section className="performance-chart-grid" aria-label="性能图表">
        <ChartPanel title="延迟分位数（毫秒）" description="按场景比较 P50、P95 和 P99">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={chartData.latency} margin={{ top: 12, right: 16, left: 0, bottom: 8 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#dce2de" />
              <XAxis dataKey="name" tick={{ fontSize: 10 }} />
              <YAxis tick={{ fontSize: 10 }} />
              <Tooltip formatter={(value) => formatMetric(typeof value === "number" ? value : null, "ms")} />
              <Legend wrapperStyle={{ fontSize: 10 }} />
              <Bar dataKey="p50" name="P50" fill="#22c979" />
              <Bar dataKey="p95" name="P95" fill="#e3a008" />
              <Bar dataKey="p99" name="P99" fill="#c2410c" />
            </BarChart>
          </ResponsiveContainer>
        </ChartPanel>
        <ChartPanel title="并发与吞吐" description="横轴为场景，折线同时标示并发和请求吞吐">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={chartData.throughput} margin={{ top: 12, right: 16, left: 0, bottom: 8 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#dce2de" />
              <XAxis dataKey="name" tick={{ fontSize: 10 }} />
              <YAxis yAxisId="left" tick={{ fontSize: 10 }} />
              <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 10 }} />
              <Tooltip />
              <Legend wrapperStyle={{ fontSize: 10 }} />
              <Line yAxisId="left" type="monotone" dataKey="concurrency" name="并发" stroke="#087a45" strokeWidth={2} dot={{ r: 3 }} />
              <Line yAxisId="right" type="monotone" dataKey="throughput" name="吞吐 req/s" stroke="#2563eb" strokeWidth={2} dot={{ r: 3 }} connectNulls={false} />
            </LineChart>
          </ResponsiveContainer>
        </ChartPanel>
        <ChartPanel title="HTTP 状态码" description="包含 429 背压和其他失败响应">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={chartData.statuses} margin={{ top: 12, right: 16, left: 0, bottom: 8 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#dce2de" />
              <XAxis dataKey="status" tick={{ fontSize: 10 }} />
              <YAxis allowDecimals={false} tick={{ fontSize: 10 }} />
              <Tooltip />
              <Legend wrapperStyle={{ fontSize: 10 }} />
              <Bar dataKey="count" name="请求数" fill="#087a45" />
            </BarChart>
          </ResponsiveContainer>
        </ChartPanel>
      </section>

      <section className="performance-table-wrap" aria-label="压测场景明细">
        <table className="performance-table">
          <caption>场景明细</caption>
          <thead><tr><th>场景</th><th>请求 / 并发</th><th>成功率</th><th>P50</th><th>P95</th><th>P99</th><th>吞吐</th><th>429</th></tr></thead>
          <tbody>{report.scenarios.map((scenario) => (
            <tr key={scenario.name}>
              <th scope="row">{scenario.name}</th>
              <td>{scenario.requests.toLocaleString("zh-CN")} / {scenario.concurrency}</td>
              <td>{scenario.requests ? `${((scenario.success / scenario.requests) * 100).toFixed(1)}%` : "未采集"}</td>
              <td>{formatMetric(scenario.latency_ms.p50, "ms")}</td>
              <td>{formatMetric(scenario.latency_ms.p95, "ms")}</td>
              <td>{formatMetric(scenario.latency_ms.p99, "ms")}</td>
              <td>{formatMetric(scenario.throughput_rps, "req/s")}</td>
              <td>{(scenario.status_counts["429"] ?? 0).toLocaleString("zh-CN")}</td>
            </tr>
          ))}</tbody>
        </table>
      </section>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="performance-metric"><span>{label}</span><strong>{value}</strong></div>;
}

function ChartPanel({ title, description, children }: { title: string; description: string; children: React.ReactNode }) {
  return <section className="performance-chart-panel"><header><h2>{title}</h2><p>{description}</p></header><div className="performance-chart">{children}</div></section>;
}
