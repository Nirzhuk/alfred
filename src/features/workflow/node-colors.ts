const NODE_ACCENTS: Record<string, string> = {
  input: "input",
  prompt: "input",
  identity: "identity",
  agent: "agent",
  customAgent: "custom-agent",
  appAction: "app-action",
  chooseOutput: "output",
  memory: "memory",
  template: "template",
  fileInject: "file-inject",
  gitStatus: "git-status",
  shell: "shell",
  script: "script",
  http: "http",
  notify: "notify",
  writeFile: "write-file",
  gitHost: "git-host",
};

export function nodeAccentColor(type?: string): string {
  const accent = type && Object.prototype.hasOwnProperty.call(NODE_ACCENTS, type)
    ? NODE_ACCENTS[type]
    : undefined;
  return accent ? `var(--node-accent-${accent})` : "var(--accent)";
}
