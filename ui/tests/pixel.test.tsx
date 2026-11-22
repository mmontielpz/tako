import { describe, expect, it } from "vitest";
import { Tako } from "../src/pixel";
import { poseGrid } from "../src/pixelGrid";
import { mount } from "./helpers/dom";

describe("pixel Tako", () => {
  const poses = ["idle", "working", "waiting", "alert"] as const;

  it("every pose is a 16x16 grid using known colors", () => {
    for (const p of poses) {
      const g = poseGrid(p);
      expect(g).toHaveLength(16);
      for (const row of g) {
        expect(row).toHaveLength(16);
        expect(row).toMatch(/^[.DPLWKMYR]+$/);
      }
    }
  });

  it("poses are visually distinct", () => {
    const grids = poses.map((p) => poseGrid(p).join("/"));
    expect(new Set(grids).size).toBe(4);
  });

  it("renders an accessible, crisp svg per pose", () => {
    for (const p of poses) {
      const m = mount(<Tako pose={p} />);
      const svg = m.el.querySelector("svg")!;
      expect(svg.getAttribute("data-pose")).toBe(p);
      expect(svg.getAttribute("role")).toBe("img");
      expect(svg.getAttribute("aria-label")).toMatch(/^Tako, /);
      expect(svg.getAttribute("shape-rendering")).toBe("crispEdges");
      expect(svg.querySelectorAll("rect").length).toBeGreaterThan(60);
      m.unmount();
    }
  });
});
