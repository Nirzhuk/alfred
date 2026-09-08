import type { Node, NodeProps } from "@xyflow/react";
import { Icon } from "../../../../components/icon";
import type {
  CustomAgentNodeData,
  FileInjectNodeData,
  GitHostNodeData,
  GitStatusNodeData,
  HttpNodeData,
  NotifyNodeData,
  ScriptNodeData,
  ShellNodeData,
  TemplateNodeData,
  WriteFileNodeData,
} from "../../types";
import { previewLine, SimpleStepNode } from "../simple-step-node/simple-step-node";

export function CustomAgentNode({
  id,
  data,
}: NodeProps<Node<CustomAgentNodeData, "customAgent">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-custom-agent"
      icon={<Icon name="terminal-window" size={16} />}
      title={data.label || "Custom agent"}
      body={previewLine(data.command, "Set a command")}
      meta={data.promptMode === "stdin" ? "Prompt on stdin" : "{{prompt}} in command"}
    />
  );
}

export function TemplateNode({
  id,
  data,
}: NodeProps<Node<TemplateNodeData, "template">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-template"
      icon={<Icon name="note-pencil" size={16} />}
      title={data.label || "Template"}
      body={previewLine(data.template, "Empty template")}
      meta={data.mode === "replace" ? "Replace context" : "Append to context"}
    />
  );
}

export function FileInjectNode({
  id,
  data,
}: NodeProps<Node<FileInjectNodeData, "fileInject">>) {
  const n = data.paths?.length ?? 0;
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-file-inject"
      icon={<Icon name="file" size={16} />}
      title={data.label || "File inject"}
      body={n > 0 ? `${n} path${n === 1 ? "" : "s"}` : "No paths"}
      meta="Inject into context"
    />
  );
}

export function GitStatusNode({
  id,
  data,
}: NodeProps<Node<GitStatusNodeData, "gitStatus">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-git-status"
      icon={<Icon name="git-branch" size={16} />}
      title={data.label || "Git status"}
      body={data.includeDiff ? "Status + diff" : "Status only"}
      meta="Snapshot into context"
    />
  );
}

export function ShellNode({
  id,
  data,
}: NodeProps<Node<ShellNodeData, "shell">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-shell"
      icon={<Icon name="terminal-window" size={16} />}
      title={data.label || "Shell"}
      body={previewLine(data.command, "No command")}
      meta={data.appendOutput ? "Append stdout to context" : "Run only"}
    />
  );
}

export function ScriptNode({
  id,
  data,
}: NodeProps<Node<ScriptNodeData, "script">>) {
  const body =
    data.source === "file"
      ? previewLine(data.path, "No script file")
      : previewLine(data.body, "Empty script");
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-script"
      icon={<Icon name="code" size={16} />}
      title={data.label || "Script"}
      body={body}
      meta={data.appendOutput ? "Append stdout to context" : "Run only"}
    />
  );
}

export function HttpNode({ id, data }: NodeProps<Node<HttpNodeData, "http">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-http"
      icon={<Icon name="globe" size={16} />}
      title={data.label || "HTTP"}
      body={previewLine(
        data.url ? `${data.method} ${data.url}` : "",
        "No URL",
      )}
      meta="Response → context"
    />
  );
}

export function NotifyNode({
  id,
  data,
}: NodeProps<Node<NotifyNodeData, "notify">>) {
  const parts: string[] = [];
  if (data.desktop) parts.push("Desktop");
  if (data.webhookUrl?.trim()) parts.push("Webhook");
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-notify"
      icon={<Icon name="bell" size={16} />}
      title={data.label || "Notify"}
      body={previewLine(data.title || data.body, "Notification")}
      meta={parts.length > 0 ? parts.join(" · ") : "Disabled"}
    />
  );
}

export function WriteFileNode({
  id,
  data,
}: NodeProps<Node<WriteFileNodeData, "writeFile">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-write-file"
      icon={<Icon name="file-plus" size={16} />}
      title={data.label || "Write file"}
      body={previewLine(data.path, "No path")}
      meta="Persist to disk"
    />
  );
}

export function GitHostNode({
  id,
  data,
}: NodeProps<Node<GitHostNodeData, "gitHost">>) {
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-git-host"
      icon={<Icon name="git-pull-request" size={16} />}
      title={data.label || (data.action === "issue" ? "Open issue" : "Create PR")}
      body={previewLine(data.title, data.action === "issue" ? "New issue" : "New pull request")}
      meta={data.repo?.trim() || "working directory repo"}
    />
  );
}
