export type ParameterTarget = { kind: string; nodeId: string | null; field: string };

export type SharingParameter = {
  key: string;
  label: string;
  description?: string;
  kind: string;
  required: boolean;
  allowedProvider?: string | null;
  allowedAction?: string | null;
  target: ParameterTarget;
  targets?: ParameterTarget[];
};

export type WorkflowTemplate = {
  format: "alfred.workflow-template";
  schemaVersion: 1;
  minAlfredVersion: string;
  title: string;
  description: string;
  graph: { nodes: Array<{ id: string; type: string; data: Record<string, unknown>; position: { x: number; y: number } }>; edges: Array<{ id: string; source: string; target: string }> };
  parameters: SharingParameter[];
  requirements: Array<{ kind: string; name: string; provider?: string | null; capability?: string | null }>;
  starts: Array<{ kind: string; config: Record<string, unknown> }>;
};

export type Publication = { shareId: string; url: string; digest: string; createdAt?: string; revokedAt?: string | null };
export type PendingPublication = { requestId: string; workflowId?: string | null; digest: string; createdAt: string };
export type PublicationDraft = { title: string; description: string; template: WorkflowTemplate; removed: string[]; requirements: string[] };
export type SharedTemplate = { format: "alfred.workflow-template"; digest: string; template: WorkflowTemplate };
