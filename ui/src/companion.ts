import type { ActionId, BridgeErrorShape, MissionView } from "./types";

export type Pose = "idle" | "working" | "waiting" | "alert";

export interface CompanionSay {
  pose: Pose;
  headline: string;
  detail: string;
  /** The single most useful next step, in words. */
  next: string;
  /** Evidence id backing any success wording; absent unless evidence supports it. */
  evidenceId: string | null;
}

export interface CompanionContext {
  hasProject: boolean;
  view: MissionView | null;
  error: BridgeErrorShape | null;
  recovered: number;
}

const ACTION_TEXT: Record<ActionId, string> = {
  define_plan: "Describe the scripted agent's plan",
  approve_plan: "Review the plan and approve the permissions you are willing to grant",
  start_execution: "Start the run",
  start_verification: "Run verification",
  cancel: "Cancel if this is taking too long",
  recover: "Recover this mission",
  accept: "Accept the verified change, or reject it",
  reject: "Reject and discard the workspace",
};

export function primaryAction(view: MissionView | null): string {
  const a = view?.next_actions[0];
  return a ? ACTION_TEXT[a] : "Nothing to do here.";
}

const ERROR_TEXT: Record<string, string> = {
  busy: "Another operation is already running on this mission. Wait for it to finish or cancel it.",
  unsafe_repository:
    "This repository's Git configuration can make Git run programs (filters, merge drivers, includes). TAKO refuses to touch it. Remove those settings or use a different project.",
  isolation_required: "AI providers are disabled until a real sandbox exists (L1). Only scripts you wrote can run, at L0.",
  not_approved: "Nothing runs without your approval of this exact plan.",
  unauthorized: "A permission the run needs was not granted. Nothing was started.",
  evidence_not_acceptable: "The evidence does not allow accepting this change.",
  conflict: "The change conflicts with newer work in the project. The project was left untouched.",
  dirty_project: "The project has uncommitted changes. Commit or stash them, then accept again.",
};

const ERROR_NEXT: Record<string, string> = {
  busy: "Wait for the other operation to finish, or cancel it.",
  unsafe_repository: "Open a different project, or remove those Git settings first.",
  isolation_required: "Use the scripted agent instead.",
  unauthorized: "Define the plan again and tick the permissions the run needs.",
  not_approved: "Review and approve the plan.",
  evidence_not_acceptable: "Run verification again, or reject the change.",
  conflict: "Reject this change, or update the project and verify again.",
  dirty_project: "Commit or stash your changes, then accept again.",
};

/**
 * What Tako says, derived only from the real mission view. Success wording appears only when
 * the engine's evidence for the latest verification is valid, passing and acceptable.
 */
export function companionFor(ctx: CompanionContext): CompanionSay {
  const { view, error } = ctx;
  if (error) {
    return {
      pose: "alert",
      headline: error.code === "busy" ? "Hold on, that mission is busy." : "I have to stop here.",
      detail: ERROR_TEXT[error.code] ?? error.message,
      next: ERROR_NEXT[error.code] ?? (view ? primaryAction(view) : "Open a project to continue."),
      evidenceId: null,
    };
  }
  if (!ctx.hasProject) {
    return {
      pose: ctx.recovered > 0 ? "alert" : "idle",
      headline: ctx.recovered > 0 ? `I recovered ${ctx.recovered} interrupted mission(s).` : "Hi, I'm Tako.",
      detail: "Open a Git project and I will help you plan, run and verify a change. I propose; you decide.",
      next: "Open a Git project.",
      evidenceId: null,
    };
  }
  if (!view) {
    return {
      pose: "waiting",
      headline: "Project open.",
      detail: "Describe the change you want and how we will know it worked (acceptance criteria and a check command).",
      next: "Define a goal.",
      evidenceId: null,
    };
  }
  const m = view.mission;
  const ev = view.evidence;
  const next = primaryAction(view);
  const base = { next, evidenceId: null as string | null };
  const jobErr = view.job.error;
  if (jobErr) {
    return { pose: "alert", headline: "The last operation did not complete.", detail: ERROR_TEXT[jobErr.code] ?? jobErr.message, ...base };
  }
  if (view.job.running) {
    return {
      pose: "working",
      headline: view.job.kind === "verification" ? "Verification is running." : "The scripted agent is running.",
      detail: "I'm watching the real process. You can cancel at any time.",
      ...base,
    };
  }
  switch (m.state) {
    case "created":
      return { pose: "waiting", headline: "Goal recorded.", detail: "Next I need a plan: the script the agent will run.", ...base };
    case "plan_ready": {
      const perms = view.plan?.body.requested_permissions ?? [];
      return {
        pose: "waiting",
        headline: "A plan needs your approval.",
        detail: `It requests ${perms.length} permission(s): ${perms.join(", ") || "none"}. None are granted until you tick them. The script that will run is shown verbatim.`,
        ...base,
      };
    }
    case "approved": {
      const g = m.approval?.granted ?? [];
      return {
        pose: "waiting",
        headline: "Approved. Ready to run.",
        detail: g.length ? `Granted: ${g.join(", ")}.` : "No permissions were granted, so the run will be refused. Define the plan again to grant them.",
        ...base,
      };
    }
    case "executing":
    case "verifying":
      return { pose: "working", headline: "Working.", detail: "The engine reports this mission is running, but no worker is attached to this window.", ...base };
    case "sealed":
      return {
        pose: "waiting",
        headline: "The agent finished; its changes are sealed.",
        detail: `It exited with code ${view.execution?.exit_code ?? "?"} and changed ${view.execution?.files_changed.length ?? 0} file(s). That is the agent's own account, not verification.`,
        ...base,
      };
    case "execution_failed":
      return { pose: "alert", headline: "The run failed.", detail: m.state_reason ?? "The agent did not complete.", ...base };
    case "timed_out":
      return { pose: "alert", headline: "The run timed out.", detail: "The process group was stopped.", ...base };
    case "cancelled":
      return { pose: "alert", headline: "The run was cancelled.", detail: "Partial work is kept for inspection. You can reject it to clean up.", ...base };
    case "interrupted":
      return { pose: "alert", headline: "The run was interrupted.", detail: "The app or engine stopped mid-run. Recovery marked it interrupted; inspect the partial work or reject it.", ...base };
    case "evidence_ready": {
      if (!ev) return { pose: "alert", headline: "No evidence found.", detail: "The mission says evidence is ready but none could be read.", ...base };
      if (!ev.valid) {
        return {
          pose: "alert",
          headline: "Verification evidence is invalid.",
          detail: `Integrity problems: ${ev.integrity_violations.join(", ")}. Check results are not trustworthy and accepting is blocked.`,
          ...base,
        };
      }
      if (ev.result !== "pass" || !ev.acceptable) {
        return {
          pose: "alert",
          headline: "Verification failed.",
          detail: `${ev.failed_checks.length} check(s) failed. Accepting is blocked. Evidence ${ev.id}.`,
          ...base,
          evidenceId: ev.id,
        };
      }
      return {
        pose: "waiting",
        headline: "Verification passed.",
        detail: `All ${ev.checks.length} check(s) exited 0 on a clean checkout and the subject was unchanged (evidence ${ev.id}). The decision is yours.`,
        ...base,
        evidenceId: ev.id,
      };
    }
    case "accepted":
      if (!ev?.valid || !ev.acceptable || ev.result !== "pass"
        || view.decision?.decision !== "accept" || view.decision.evidence_id !== ev.id
        || view.decision.result_tree_matches_verified !== true) {
        return { pose: "alert", headline: "Acceptance is unverified.", detail: "Matching evidence and a recorded decision are required.", ...base };
      }
      return {
        pose: "idle",
        headline: "Accepted.",
        detail: `You accepted this change on the basis of evidence ${view.decision?.evidence_id ?? "(none recorded)"}.`,
        ...base,
        evidenceId: view.decision?.evidence_id ?? null,
      };
    case "rejected":
      return { pose: "idle", headline: "Rejected.", detail: "The workspace was discarded; your project is unchanged. Evidence is kept.", ...base };
  }
}
