import { listen } from "@tauri-apps/api/event";
import type { WorkflowGroupDefinition, WorkflowRole, WorkflowRoleContract, WorkflowRun } from "$lib/domain";
import {
  advanceWorkflowRun,
  approveWorkflowHandoff,
  cancelWorkflowRun,
  loadWorkflowRoleContract,
  loadWorkflowRun,
  pauseWorkflowRun,
  previewWorkflowContext,
  resumeWorkflowRun,
  retryWorkflowStep,
  skipWorkflowStep,
  startWorkflowRun,
} from "$lib/lume";

/**
 * What the workflow board asks of Lume. On the desktop it is the app itself; Lume Mobile passes one that
 * sends the same requests through the paired gateway, so both run the same board.
 */
export type WorkflowBoardBackend = {
  roleContract(role: WorkflowRole): Promise<WorkflowRoleContract>;
  previewContext(group: WorkflowGroupDefinition, connectionId: string, objective: string, sourceResultId?: string): ReturnType<typeof previewWorkflowContext>;
  loadRun(workflowId: string): Promise<WorkflowRun | null>;
  startRun(group: WorkflowGroupDefinition, objective: string): Promise<WorkflowRun>;
  approve(workflowId: string): Promise<WorkflowRun>;
  advance(workflowId: string): Promise<WorkflowRun>;
  pause(workflowId: string): Promise<WorkflowRun>;
  resume(workflowId: string): Promise<WorkflowRun>;
  retry(workflowId: string): Promise<WorkflowRun>;
  skip(workflowId: string): Promise<WorkflowRun>;
  cancel(workflowId: string): Promise<WorkflowRun>;
  /** Calls back with every run that changes; resolves to the function that stops listening. */
  onRunChanged(callback: (run: WorkflowRun) => void): Promise<() => void>;
};

export const desktopWorkflowBackend: WorkflowBoardBackend = {
  roleContract: loadWorkflowRoleContract,
  previewContext: previewWorkflowContext,
  loadRun: loadWorkflowRun,
  startRun: startWorkflowRun,
  approve: approveWorkflowHandoff,
  advance: advanceWorkflowRun,
  pause: pauseWorkflowRun,
  resume: resumeWorkflowRun,
  retry: retryWorkflowStep,
  skip: skipWorkflowStep,
  cancel: cancelWorkflowRun,
  onRunChanged: (callback) => listen<WorkflowRun>("lume://workflow-run-changed", ({ payload }) => callback(payload)),
};
