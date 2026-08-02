import { QueryClient, QueryClientProvider, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes, useNavigate } from "react-router-dom";

import { AppShell } from "./components/app-shell.js";
import { api, ApiError } from "./lib/api.js";
import { GetStartedPage } from "./pages/get-started-page.js";
import { LoginPage } from "./pages/login-page.js";
import { LandingPage } from "./pages/landing-page.js";
import { MemoryLabPage } from "./pages/memory-lab-page.js";
import { OverviewPage } from "./pages/overview-page.js";
import { CapabilitiesPage } from "./pages/capabilities-page.js";
import { RuntimePage } from "./pages/runtime-page.js";

const PerformancePage = lazy(() =>
  import("./pages/performance-page.js").then((module) => ({ default: module.PerformancePage })),
);

export function App() {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: false, staleTime: 15_000 },
          mutations: { retry: false },
        },
      }),
  );
  return (
    <QueryClientProvider client={client}>
      <BrowserRouter>
        <AppRoutes />
      </BrowserRouter>
    </QueryClientProvider>
  );
}

function AppRoutes() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const session = useQuery({ queryKey: ["session"], queryFn: api.session });
  const login = useMutation({
    mutationFn: api.login,
    onSuccess(data) {
      queryClient.setQueryData(["session"], data);
      navigate("/instances", { replace: true });
    },
  });

  if (session.isPending) {
    return <div className="app-loading" role="status">正在检查控制面会话</div>;
  }
  if (session.isError) {
    return <Routes>
      <Route path="/" element={<LandingPage />} />
      <Route path="/login" element={<LoginPage
          pending={login.isPending}
          error={login.error instanceof ApiError ? login.error.message : login.isError ? "无法连接控制面" : null}
          onLogin={async (password) => {
            await login.mutateAsync(password);
          }}
        />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>;
  }

  return <AuthenticatedApp />;
}

function AuthenticatedApp() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const instances = useQuery({ queryKey: ["instances"], queryFn: api.instances });
  const logout = useMutation({
    mutationFn: api.logout,
    onSuccess() {
      queryClient.clear();
      navigate("/", { replace: true });
      window.location.reload();
    },
  });
  if (instances.isPending) return <div className="app-loading" role="status">正在读取实例连接</div>;
  if (instances.isError) return <div className="app-loading" role="alert">无法读取实例连接</div>;

  return (
    <AppShell instances={instances.data} onLogout={() => logout.mutate()}>
      <Routes>
        <Route path="/instances" element={<GetStartedPage instances={instances.data} />} />
        {instances.data.map((instance) => (
          <Route key={`${instance.id}-overview`} path={`/instances/${instance.id}/overview`} element={<OverviewPage instance={instance} />} />
        ))}
        {instances.data.map((instance) => (
          <Route key={`${instance.id}-memory`} path={`/instances/${instance.id}/memory`} element={<MemoryLabPage instance={instance} />} />
        ))}
        {instances.data.map((instance) => (
          <Route key={`${instance.id}-runtime`} path={`/instances/${instance.id}/runtime`} element={<RuntimePage instance={instance} />} />
        ))}
        {instances.data.map((instance) => (
          <Route
            key={`${instance.id}-performance`}
            path={`/instances/${instance.id}/performance`}
            element={<Suspense fallback={<div className="app-loading" role="status">正在加载性能图表</div>}><PerformancePage instance={instance} /></Suspense>}
          />
        ))}
        <Route path="/capabilities" element={<CapabilitiesPage />} />
        <Route path="*" element={<Navigate to={instances.data[0] ? `/instances/${instances.data[0].id}/overview` : "/instances"} replace />} />
      </Routes>
    </AppShell>
  );
}
