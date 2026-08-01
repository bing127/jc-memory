import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";

import { App } from "../src/app.js";

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState({}, "", "/");
});

test("public entry connects the complete documented product story, 3D system model and login", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json({ code: "unauthorized" }, { status: 401 })),
  );
  const user = userEvent.setup();

  const { container } = render(<App />);

  expect(await screen.findByRole("heading", { name: "疆诚记忆" })).not.toBeNull();
  expect(screen.getByRole("navigation", { name: "公开导航" })).not.toBeNull();
  expect(screen.getByRole("link", { name: "产品" })).not.toBeNull();
  expect(screen.getByRole("link", { name: "Runtime" })).not.toBeNull();
  expect(screen.getByRole("link", { name: "安全治理" })).not.toBeNull();
  expect(screen.getByRole("link", { name: "部署" })).not.toBeNull();
  expect(screen.getByRole("link", { name: "定价" })).not.toBeNull();
  const universe = screen.getByRole("figure", { name: "疆诚记忆三维证据网络" });
  expect(universe).not.toBeNull();
  expect(universe.textContent).toContain("AUTHORITY RECORDS");
  expect(universe.textContent).toContain("DERIVED GRAPH");
  expect(universe.textContent).toContain("ACTIVE SURFACES");
  expect(container.querySelector("canvas[data-memory-universe]")).not.toBeNull();
  const hero = container.querySelector("#product");
  expect(Array.from(hero?.children ?? []).some((element) => element.className.includes("md:w-[58%]"))).toBe(false);

  expect(screen.getByRole("heading", { name: "从原始证据，到可恢复的行动闭环。" })).not.toBeNull();
  expect(screen.getByLabelText("记忆层级")).not.toBeNull();
  const chain = screen.getByLabelText("Agent Runtime 完整链路");
  for (const stage of ["Observe", "Recall", "Memory Packet", "Model", "Policy", "Approval", "Tool", "Commit"]) {
    expect(chain.textContent).toContain(stage);
  }
  expect(screen.getByRole("heading", { name: "十种 Recall 模式，同一份证据边界。" })).not.toBeNull();
  expect(screen.getByLabelText("Memory Packet 工作区预览")).not.toBeNull();
  expect(screen.getByText("Recall Trace")).not.toBeNull();
  expect(screen.getByText("Point / Edge / Surface")).not.toBeNull();
  expect(screen.getByText("SQLite WAL / FTS5")).not.toBeNull();
  expect(screen.getByText("Embedded / Sidecar / Server / MCP")).not.toBeNull();

  expect(screen.getByRole("heading", { name: "Self-hosted" })).not.toBeNull();
  expect(screen.getByRole("heading", { name: "Managed Cloud" })).not.toBeNull();
  expect(screen.getByRole("heading", { name: "Enterprise" })).not.toBeNull();
  expect(screen.queryByText(/\$\d+|¥\d+|每席位|每月/)).toBeNull();

  await user.click(screen.getByRole("link", { name: "Get Started" }));
  expect(await screen.findByRole("heading", { name: "进入控制台" })).not.toBeNull();
  expect(window.location.pathname).toBe("/login");
});
