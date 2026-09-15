import { invoke } from "@tauri-apps/api/core";
import type { PendingPublication, Publication, PublicationDraft, SharedTemplate } from "./types";
import type { AgentAccount } from "../agent-accounts/types";
import type { AppConnection } from "../integrations/types";

export type SharingInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export function sharingErrorMessage(reason: unknown, fallback: string): string {
  const message = reason instanceof Error ? reason.message : String(reason ?? "");
  if (message.toLowerCase().includes("sharing service is not configured")) {
    return "Workflow sharing is not configured for this Alfred build. Local workflows remain available; ask your administrator to configure the sharing service.";
  }
  return message || fallback;
}

export function createSharingApi(call: SharingInvoke = invoke) {
  return {
    preparePublication(workflowId: string) { return call<PublicationDraft>("prepare_workflow_publication", { workflowId }); },
    publishPublication(input: { workflowId: string; requestId: string; template: PublicationDraft["template"] }) { return call<Publication>("publish_workflow_template", { input }); },
    listPublications(workflowId?: string) { return call<Publication[]>("list_workflow_publications", { workflowId: workflowId ?? null }); },
    listPendingPublications() { return call<PendingPublication[]>("list_pending_workflow_publications"); },
    resumePublication(requestId: string) { return call<Publication>("resume_workflow_publication", { requestId }); },
    listAgentAccounts() { return call<AgentAccount[]>("list_agent_accounts"); },
    listAppConnections() { return call<AppConnection[]>("list_app_connections"); },
    revokePublication(shareId: string) { return call<void>("revoke_workflow_publication", { shareId }); },
    resolveLink(link: string) { return call<SharedTemplate>("resolve_shared_workflow_link", { link }); },
    checkSetup(input: { template: SharedTemplate["template"]; values: Record<string, unknown> }) { return call<{ complete: boolean; errors: string[] }>("check_workflow_template_setup", { input }); },
    importTemplate(input: { template: SharedTemplate["template"]; values: Record<string, unknown>; sourceLink: string; digest: string; importAttemptId: string }) { return call<{ workflowId: string; existingCopy?: boolean }>("import_workflow_template", { input }); },
  };
}

export const sharingApi = createSharingApi();
