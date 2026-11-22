import type { Pose } from "./companion";

// 16x16 pixel-art Tako. Letters map to colors; '.' is transparent. Drawn for this project.
const BODY = [
  "....DDDDDDDD....",
  "...DPPPPPPPPD...",
  "..DPPLLPPPPPPD..",
  "..DPLLPPPPPPPD..",
  "..DPPPPPPPPPPD..",
  "..DPPPPPPPPPPD..", // 5: eyes row 1
  "..DPPPPPPPPPPD..", // 6: eyes row 2
  "..DPPPPPPPPPPD..",
  "..DPPPPPPPPPPD..", // 8: mouth
  "...DPPPPPPPPD...",
  "..DPDPPDDPPDPD..",
  ".DPPDPDPPDPDPPD.",
  ".DPD.DPD.DPD.DPD",
  ".DP..DP...PD..PD",
  "..D...D...D...D.",
  "................",
];

export const PALETTE: Record<string, string> = {
  D: "#5B2FB8",
  P: "#8B5CF6",
  L: "#B07CFF",
  W: "#FFFFFF",
  K: "#14101F",
  M: "#14101F",
  Y: "#F2C14E",
  R: "#FF6B7A",
};

function set(rows: string[][], r: number, c: number, ch: string) {
  const row = rows[r];
  if (row) row[c] = ch;
}

export function poseGrid(pose: Pose): string[] {
  const g = BODY.map((r) => r.split(""));
  // eyes
  if (pose === "alert") {
    for (const c of [5, 6, 9, 10]) for (const r of [4, 5, 6]) set(g, r, c, "W");
    for (const c of [6, 9]) set(g, 5, c, "K");
    for (const c of [5, 10]) set(g, 3, c, "R"); // raised brows
  } else if (pose === "working") {
    for (const c of [5, 6, 9, 10]) set(g, 5, c, "W");
    for (const c of [5, 6, 9, 10]) set(g, 6, c, "K"); // looking down
    set(g, 1, 13, "Y"); set(g, 0, 14, "Y"); set(g, 2, 14, "Y"); // sparks
  } else if (pose === "waiting") {
    for (const c of [5, 6, 9, 10]) { set(g, 5, c, "W"); set(g, 6, c, "W"); }
    for (const c of [6, 10]) set(g, 5, c, "K"); // looking up-right
    set(g, 1, 14, "Y"); set(g, 2, 14, "Y"); set(g, 4, 14, "Y"); // '?' dots
    set(g, 0, 13, "Y"); set(g, 0, 14, "Y");
  } else {
    for (const c of [5, 6, 9, 10]) { set(g, 5, c, "W"); set(g, 6, c, "W"); }
    for (const c of [5, 9]) set(g, 6, c, "K");
  }
  // mouth
  if (pose === "alert") { set(g, 8, 7, "M"); set(g, 8, 8, "M"); set(g, 9, 7, "M"); set(g, 9, 8, "M"); }
  else if (pose === "working") { set(g, 8, 6, "M"); set(g, 8, 7, "M"); set(g, 8, 8, "M"); set(g, 8, 9, "M"); }
  else { set(g, 8, 6, "M"); set(g, 9, 7, "M"); set(g, 9, 8, "M"); set(g, 8, 9, "M"); }
  return g.map((r) => r.join(""));
}

