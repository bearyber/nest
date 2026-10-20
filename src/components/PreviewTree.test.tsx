// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Plan } from "../lib/types";
import PreviewTree from "./PreviewTree";

const plan = {
  folderName: "261020_TEST",
  folders: ["01_FOOTAGE", "01_FOOTAGE/A_CAM", "02_AUDIO", "06_DI", "06_DI/EXPORTS"],
  files: [],
} as unknown as Plan;

const openCount = () => screen.getAllByRole("treeitem").filter((t) => t.getAttribute("aria-expanded") === "true").length;

describe("PreviewTree fold button", () => {
  it("folds the top folders, then opens them all again on the next press", () => {
    render(<PreviewTree plan={plan} />);
    const before = openCount();
    fireEvent.click(screen.getByRole("button", { name: "Collapse all folders" }));
    expect(openCount()).toBeLessThan(before);
    fireEvent.click(screen.getByRole("button", { name: "Open all folders" }));
    expect(openCount()).toBe(before);
    expect(screen.getByRole("button", { name: "Collapse all folders" })).toBeTruthy();
  });
});
