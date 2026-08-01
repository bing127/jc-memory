import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import {
  CapabilitiesPage,
  capabilityRegistry,
} from "../src/pages/capabilities-page.js";

test("only LIVE capabilities can enter production navigation", () => {
  const navigable = capabilityRegistry.filter((capability) => capability.productionNavigation);
  expect(navigable.length).toBeGreaterThan(0);
  expect(navigable.every((capability) => capability.status === "LIVE")).toBe(true);
  expect(capabilityRegistry.every((capability) => capability.evidence.length > 0)).toBe(true);
});

test("stable memory mutation operations are exposed by the live Memory Lab", () => {
  const mutations = capabilityRegistry.find(
    (capability) => capability.name === "Commit / Correct / Forget",
  );
  expect(mutations).toMatchObject({
    status: "LIVE",
    surface: "Memory Lab",
    productionNavigation: true,
  });
});

test("renders roadmap states without fake execution actions", () => {
  render(<CapabilitiesPage />);

  expect(screen.getAllByText("LIVE").length).toBeGreaterThan(0);
  expect(screen.getAllByText("CORE").length).toBeGreaterThan(0);
  expect(screen.getAllByText("ROADMAP").length).toBeGreaterThan(0);
  expect(screen.getAllByText("PLATFORM").length).toBeGreaterThan(0);
  expect(screen.getByText("Managed Sandbox")).not.toBeNull();
  expect(screen.queryByRole("button", { name: /创建 Sandbox|立即启用/ })).toBeNull();
});
