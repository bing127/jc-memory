import { Activity, Boxes, BrainCircuit, Database, Gauge, LogOut, Menu, Server, Workflow, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";

import type { InstanceSummary } from "../lib/contracts.js";
import { Button } from "./ui/button.js";

interface AppShellProps {
  instances: InstanceSummary[];
  children: ReactNode;
  onLogout(): void;
}

export function AppShell({ instances, children, onLogout }: AppShellProps) {
  const [mobileOpen, setMobileOpen] = useState(false);
  const location = useLocation();
  const navigate = useNavigate();
  const selected = instances.find((instance) => location.pathname.includes(instance.id));
  const basePath = selected ? `/instances/${selected.id}` : null;

  return (
    <div className="console-layout">
      <header className="console-mobile-header">
        <Brand />
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label={mobileOpen ? "关闭导航" : "打开导航"}
          onClick={() => setMobileOpen((value) => !value)}
        >
          {mobileOpen ? <X aria-hidden="true" /> : <Menu aria-hidden="true" />}
        </Button>
      </header>
      <aside className={`console-sidebar ${mobileOpen ? "is-open" : ""}`}>
        <Brand />
        <div className="instance-switcher">
          <label htmlFor="instance-switch">当前实例</label>
          <select
            id="instance-switch"
            value={selected?.id ?? ""}
            onChange={(event) =>
              navigate(event.target.value ? `/instances/${event.target.value}/overview` : "/instances")
            }
          >
            <option value="">连接或选择实例</option>
            {instances.map((instance) => (
              <option key={instance.id} value={instance.id}>{instance.name}</option>
            ))}
          </select>
        </div>
        <nav className="console-nav" aria-label="控制台主导航" onClick={() => setMobileOpen(false)}>
          <NavLink to="/instances"><Database aria-hidden="true" />实例</NavLink>
          {basePath ? (
            <>
              <div className="nav-section">MEMORY</div>
              <NavLink to={`${basePath}/overview`}><Activity aria-hidden="true" />总览</NavLink>
              <NavLink to={`${basePath}/memory`}><BrainCircuit aria-hidden="true" />Memory Lab</NavLink>
              <div className="nav-section">RUNTIME</div>
              <NavLink to={`${basePath}/runtime`}><Workflow aria-hidden="true" />Agent Runtime</NavLink>
              <NavLink to={`${basePath}/performance`}><Gauge aria-hidden="true" />性能观测</NavLink>
            </>
          ) : null}
          <div className="nav-section">SYSTEM</div>
          <NavLink to="/capabilities"><Boxes aria-hidden="true" />能力矩阵</NavLink>
        </nav>
        <Button type="button" variant="ghost" className="sidebar-logout" onClick={onLogout}>
          <LogOut aria-hidden="true" size={16} />退出登录
        </Button>
      </aside>
      <div className="console-main">
        <header className="console-topbar">
          <div>
            <Server aria-hidden="true" />
            <span><small>当前实例</small><strong>{selected?.name ?? "尚未连接实例"}</strong></span>
          </div>
          {selected ? <div className="console-connection"><span aria-hidden="true" />REST {selected.protocolVersion}</div> : null}
        </header>
        {children}
      </div>
    </div>
  );
}

function Brand() {
  return (
    <div className="console-brand">
      <span className="console-brand-mark"><Boxes aria-hidden="true" /></span>
      <span><strong>疆诚记忆</strong><small>JIANGCHENG CONTROL</small></span>
    </div>
  );
}
