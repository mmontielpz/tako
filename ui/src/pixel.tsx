import type { Pose } from "./companion";
import { PALETTE, poseGrid } from "./pixelGrid";

const LABEL: Record<Pose, string> = {
  idle: "Tako, idle",
  working: "Tako, working",
  waiting: "Tako, waiting for you",
  alert: "Tako, alert",
};

export function Tako({ pose, size = 96 }: { pose: Pose; size?: number }) {
  const rects = [];
  const g = poseGrid(pose);
  for (let y = 0; y < g.length; y++) {
    const row = g[y] ?? "";
    for (let x = 0; x < row.length; x++) {
      const ch = row[x] ?? ".";
      const fill = PALETTE[ch];
      if (fill) rects.push(<rect key={`${x}-${y}`} x={x} y={y} width={1} height={1} fill={fill} />);
    }
  }
  return (
    <svg
      class="tako"
      data-pose={pose}
      role="img"
      aria-label={LABEL[pose]}
      viewBox="0 0 16 16"
      width={size}
      height={size}
      shape-rendering="crispEdges"
    >
      {rects}
    </svg>
  );
}
