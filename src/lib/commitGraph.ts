// Lays a git history out as a graph: which lane each commit sits in and which lines run through each row.
// The commits come newest first in topological order, so a parent always comes after its children.

export interface GraphCommit { oid: string; parents: string[] }

/** What to draw in one row: the dot's lane, and every line that touches the row. */
export interface GraphRow {
  lane: number;
  /** Lines that run straight through the row (a branch that is not this commit's). */
  through: number[];
  /** Lines that end at the dot from the row above: the commit's own lane and any branch that merged into it here. */
  from: number[];
  /** Lines that start at the dot and go down to the lane of each parent. */
  to: number[];
  /** True when a line comes from above into the dot's own lane. */
  continues: boolean;
  /** Lanes in use after this row, for the width of the graph. */
  width: number;
}

export function layoutGraph(commits: GraphCommit[], laneLimit = 12): GraphRow[] {
  // `lanes[i]` is the commit a line in lane i is heading for, or null when the lane is free.
  let lanes: (string | null)[] = [];
  const rows: GraphRow[] = [];
  for (const commit of commits) {
    let lane = lanes.indexOf(commit.oid);
    const continues = lane !== -1;
    if (lane === -1) {
      lane = lanes.indexOf(null);
      if (lane === -1) lane = lanes.length;
      lanes[lane] = commit.oid;
    }
    // Every lane heading for this commit meets at its dot.
    const from = lanes.flatMap((target, index) => (target === commit.oid ? [index] : []));
    const through = lanes.flatMap((target, index) => (target !== null && target !== commit.oid ? [index] : []));
    for (const index of from) lanes[index] = null;
    const to: number[] = [];
    commit.parents.forEach((parent, order) => {
      // The first parent keeps the commit's lane, so a branch stays in one column, even when another line already
      // heads for the same parent (the two meet there). Other parents join a line that exists or take a free lane.
      let target = order === 0 ? lane : lanes.indexOf(parent);
      if (target === -1) {
        target = lanes.indexOf(null);
        if (target === -1) target = lanes.length;
      }
      lanes[target] = parent;
      to.push(target);
    });
    while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();
    rows.push({ lane: Math.min(lane, laneLimit), through, from, to, continues, width: Math.min(Math.max(lanes.length, lane + 1), laneLimit + 1) });
  }
  return rows;
}

/** The lane count of the widest row, so every row is drawn at the same width. */
export function graphWidth(rows: GraphRow[]): number {
  return rows.reduce((widest, row) => Math.max(widest, row.width, ...row.to.map((lane) => lane + 1), ...row.through.map((lane) => lane + 1)), 1);
}
