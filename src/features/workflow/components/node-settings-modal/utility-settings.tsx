import { useEffect, useId, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { SelectControl } from "../../../../components/select-control";
import * as api from "../../api";
import type {
  CustomAgentNodeData,
  FileInjectNodeData,
  GitHostNodeData,
  GitStatusNodeData,
  HttpNodeData,
  NotifyNodeData,
  ScriptNodeData,
  ScriptRef,
  ShellNodeData,
  TemplateNodeData,
  WriteFileNodeData,
} from "../../types";
import { defaultInterpreter } from "../../types";
import { askNames } from "../../ask-fields";

function LabelField({
  value,
  onChange,
}: {
  value: string;
  onChange: (label: string) => void;
}) {
  return (
    <label className="field">
      <span>Label</span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

export function CustomAgentSettings({
  data,
  onUpdate,
}: {
  data: CustomAgentNodeData;
  onUpdate: (patch: Partial<CustomAgentNodeData>) => void;
}) {
  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <label className="field">
        <span>Command</span>
        <textarea
          rows={4}
          value={data.command}
          placeholder='e.g. my-agent --print "{{prompt}}"'
          onChange={(e) => onUpdate({ command: e.target.value })}
        />
      </label>
      <label className="field">
        <span>Pass prompt via</span>
        <select
          value={data.promptMode}
          onChange={(e) =>
            onUpdate({
              promptMode: e.target.value as CustomAgentNodeData["promptMode"],
            })
          }
        >
          <option value="template">{"{{prompt}} in command"}</option>
          <option value="stdin">stdin</option>
        </select>
      </label>
      <p className="hint">
        Runs like a built-in agent: workflow context becomes the prompt, output
        is captured, and git file changes are tracked when a working directory
        is set.
        {data.promptMode === "template"
          ? " Include {{prompt}} in the command (quote it if needed)."
          : " The prompt is written to the process stdin."}
      </p>
    </>
  );
}

export function TemplateSettings({
  data,
  onUpdate,
}: {
  data: TemplateNodeData;
  onUpdate: (patch: Partial<TemplateNodeData>) => void;
}) {
  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <p className="hint">
        Placeholders: <code>{"{{context}}"}</code>, <code>{"{{output}}"}</code>,{" "}
        <code>{"{{cwd}}"}</code>. Use <code>{"{{ask:Name}}"}</code> or{" "}
        <code>{"{{ask:Name|default}}"}</code> to be asked when you press Run.
      </p>
      <label className="field">
        <span>Template</span>
        <textarea
          rows={8}
          value={data.template}
          onChange={(e) => onUpdate({ template: e.target.value })}
        />
      </label>
      <AskPreview text={data.template} />
      <label className="field">
        <span>Mode</span>
        <select
          value={data.mode}
          onChange={(e) =>
            onUpdate({ mode: e.target.value as TemplateNodeData["mode"] })
          }
        >
          <option value="append">Append to context</option>
          <option value="replace">Replace context</option>
        </select>
      </label>
    </>
  );
}

export function FileInjectSettings({
  data,
  onUpdate,
}: {
  data: FileInjectNodeData;
  onUpdate: (patch: Partial<FileInjectNodeData>) => void;
}) {
  const addPaths = async () => {
    try {
      const picked = await open({
        multiple: true,
        directory: false,
        title: "Inject files into context",
      });
      if (!picked) return;
      const list = Array.isArray(picked) ? picked : [picked];
      const next = [...data.paths];
      for (const path of list) {
        if (path && !next.includes(path)) next.push(path);
      }
      onUpdate({ paths: next });
    } catch (e) {
      console.warn("File picker unavailable", e);
    }
  };

  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <div className="field">
        <div className="wf-attach-field-header">
          <span>Paths</span>
          <button
            type="button"
            className="ghost wf-input-attach-btn"
            onClick={() => void addPaths()}
          >
            + File
          </button>
        </div>
        <p className="hint">File contents are appended to the run context.</p>
        {data.paths.length === 0 ? (
          <p className="muted">No paths selected.</p>
        ) : (
          <ul className="wf-path-list">
            {data.paths.map((path) => (
              <li key={path}>
                <code>{path}</code>
                <button
                  type="button"
                  className="ghost"
                  onClick={() =>
                    onUpdate({
                      paths: data.paths.filter((p) => p !== path),
                    })
                  }
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  );
}

export function GitStatusSettings({
  data,
  onUpdate,
}: {
  data: GitStatusNodeData;
  onUpdate: (patch: Partial<GitStatusNodeData>) => void;
}) {
  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.includeDiff}
          onChange={(e) => onUpdate({ includeDiff: e.target.checked })}
        />
        <span>Include git diff</span>
      </label>
      <p className="hint">
        Snapshots the workflow working directory’s git status into context
        before later steps run.
      </p>
    </>
  );
}

export function ShellSettings({
  data,
  onUpdate,
}: {
  data: ShellNodeData;
  onUpdate: (patch: Partial<ShellNodeData>) => void;
}) {
  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <label className="field">
        <span>Command</span>
        <textarea
          rows={4}
          value={data.command}
          placeholder="e.g. bun test"
          onChange={(e) => onUpdate({ command: e.target.value })}
        />
      </label>
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.appendOutput}
          onChange={(e) => onUpdate({ appendOutput: e.target.checked })}
        />
        <span>Append stdout/stderr to context</span>
      </label>
    </>
  );
}

async function pickScriptFile(): Promise<string | null> {
  try {
    const picked = await open({
      multiple: false,
      directory: false,
      title: "Choose a script",
    });
    return typeof picked === "string" ? picked : null;
  } catch (e) {
    console.warn("File picker unavailable", e);
    return null;
  }
}

/**
 * Source, path/body, and interpreter controls. Shared by the Script node and
 * the Input node's "use this script" option so both stay identical.
 */
export function ScriptRefFields({
  data,
  onUpdate,
  disabled = false,
}: {
  data: ScriptRef;
  onUpdate: (patch: Partial<ScriptRef>) => void;
  disabled?: boolean;
}) {
  return (
    <>
      <label className="field">
        <span>Source</span>
        <SelectControl
          value={data.source}
          disabled={disabled}
          onChange={(e) =>
            onUpdate({ source: e.target.value as ScriptRef["source"] })
          }
        >
          <option value="file">File on disk</option>
          <option value="inline">Saved script</option>
        </SelectControl>
      </label>

      {data.source === "file" ? (
        <div className="field">
          <div className="wf-attach-field-header">
            <span>Path</span>
            <button
              type="button"
              className="ghost wf-input-attach-btn"
              disabled={disabled}
              onClick={() => {
                void pickScriptFile().then((path) => {
                  if (path) onUpdate({ path });
                });
              }}
            >
              Browse…
            </button>
          </div>
          <input
            type="text"
            value={data.path}
            disabled={disabled}
            placeholder="e.g. ./scripts/run-tests.sh"
            onChange={(e) => onUpdate({ path: e.target.value })}
          />
        </div>
      ) : (
        <label className="field">
          <span>Script</span>
          <textarea
            className="wf-script-body"
            rows={8}
            spellCheck={false}
            value={data.body}
            disabled={disabled}
            placeholder={"#!/usr/bin/env bash\nbun test"}
            onChange={(e) => onUpdate({ body: e.target.value })}
          />
        </label>
      )}

      <label className="field">
        <span>Run with</span>
        <input
          type="text"
          value={data.interpreter}
          disabled={disabled}
          placeholder={defaultInterpreter()}
          onChange={(e) => onUpdate({ interpreter: e.target.value })}
        />
      </label>
    </>
  );
}

export function ScriptSettings({
  data,
  onUpdate,
}: {
  data: ScriptNodeData;
  onUpdate: (patch: Partial<ScriptNodeData>) => void;
}) {
  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <ScriptRefFields data={data} onUpdate={onUpdate} />
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.appendOutput}
          onChange={(e) => onUpdate({ appendOutput: e.target.checked })}
        />
        <span>Append stdout/stderr to context</span>
      </label>
      <p className="hint">
        The previous step’s output arrives on stdin and as{" "}
        <code>$ALFRED_OUTPUT</code>; the accumulated context is{" "}
        <code>$ALFRED_CONTEXT</code>. On macOS and Linux a leading{" "}
        <code>#!</code> line overrides “Run with”. A non-zero exit stops the run.
      </p>
    </>
  );
}

export function HttpSettings({
  data,
  onUpdate,
}: {
  data: HttpNodeData;
  onUpdate: (patch: Partial<HttpNodeData>) => void;
}) {
  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <div className="field-row">
        <label className="field" style={{ width: "7rem" }}>
          <span>Method</span>
          <select
            value={data.method}
            onChange={(e) =>
              onUpdate({ method: e.target.value as HttpNodeData["method"] })
            }
          >
            {(["GET", "POST", "PUT", "PATCH", "DELETE"] as const).map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </label>
        <label className="field" style={{ flex: 1 }}>
          <span>URL</span>
          <input
            type="url"
            value={data.url}
            placeholder="https://…"
            onChange={(e) => onUpdate({ url: e.target.value })}
          />
        </label>
      </div>
      <label className="field">
        <span>Headers</span>
        <textarea
          rows={3}
          value={data.headers}
          placeholder={"Authorization: Bearer …\nContent-Type: application/json"}
          onChange={(e) => onUpdate({ headers: e.target.value })}
        />
      </label>
      <label className="field">
        <span>Body</span>
        <textarea
          rows={5}
          value={data.body}
          placeholder="Templates: {{output}}, {{context}}"
          onChange={(e) => onUpdate({ body: e.target.value })}
        />
      </label>
      <p className="hint">Response body is appended to context.</p>
    </>
  );
}

export function NotifySettings({
  data,
  onUpdate,
}: {
  data: NotifyNodeData;
  onUpdate: (patch: Partial<NotifyNodeData>) => void;
}) {
  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <label className="field">
        <span>Title</span>
        <input
          type="text"
          value={data.title}
          onChange={(e) => onUpdate({ title: e.target.value })}
        />
      </label>
      <label className="field">
        <span>Body</span>
        <textarea
          rows={4}
          value={data.body}
          onChange={(e) => onUpdate({ body: e.target.value })}
        />
      </label>
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.desktop}
          onChange={(e) => onUpdate({ desktop: e.target.checked })}
        />
        <span>Desktop notification</span>
      </label>
      <label className="field">
        <span>Webhook URL (optional)</span>
        <input
          type="url"
          value={data.webhookUrl}
          placeholder="https://hooks.…"
          onChange={(e) => onUpdate({ webhookUrl: e.target.value })}
        />
      </label>
    </>
  );
}

export function WriteFileSettings({
  data,
  onUpdate,
}: {
  data: WriteFileNodeData;
  onUpdate: (patch: Partial<WriteFileNodeData>) => void;
}) {
  const pickPath = async () => {
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Write file destination",
      });
      if (typeof picked === "string") onUpdate({ path: picked });
    } catch (e) {
      console.warn("File picker unavailable", e);
    }
  };

  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <label className="field">
        <span>Path</span>
        <div className="field-row">
          <input
            type="text"
            style={{ flex: 1 }}
            value={data.path}
            placeholder="relative/or/absolute/path.txt"
            onChange={(e) => onUpdate({ path: e.target.value })}
          />
          <button type="button" className="ghost" onClick={() => void pickPath()}>
            Browse…
          </button>
        </div>
      </label>
      <label className="field">
        <span>Content</span>
        <textarea
          rows={6}
          value={data.content}
          onChange={(e) => onUpdate({ content: e.target.value })}
        />
      </label>
      <p className="hint">
        Templates: <code>{"{{output}}"}</code>, <code>{"{{context}}"}</code>,{" "}
        <code>{"{{cwd}}"}</code>
      </p>
    </>
  );
}

export function GitHostSettings({
  data,
  onUpdate,
  cwd,
}: {
  data: GitHostNodeData;
  onUpdate: (patch: Partial<GitHostNodeData>) => void;
  /** Workflow working directory — used to detect the checked-out repo. */
  cwd?: string;
}) {
  const repo = data.repo ?? "";
  const isPr = data.action === "pr";
  const suggestionsId = useId();
  const [reloadKey, setReloadKey] = useState(0);
  const [gh, setGh] = useState<api.GithubCliRepos | null>(null);
  const [branches, setBranches] = useState<{
    repo: string;
    names: string[];
    error?: string;
  } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setGh(null);
    api
      .githubCliRepos(cwd)
      .then((result) => {
        if (!cancelled) setGh(result);
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setGh({
            current: null,
            repos: [],
            error: String(e),
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [cwd, reloadKey]);

  const branchRepo = repo.trim() || gh?.current || "";
  useEffect(() => {
    setBranches(null);
    if (!isPr || !branchRepo) {
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .githubCliBranches(branchRepo)
        .then((result) => {
          if (!cancelled) setBranches({ repo: branchRepo, names: result });
        })
        .catch((e: unknown) => {
          if (!cancelled) {
            setBranches({ repo: branchRepo, names: [], error: String(e) });
          }
        });
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [isPr, branchRepo, reloadKey]);

  const currentBranches = branches?.repo === branchRepo ? branches : null;

  return (
    <>
      <LabelField
        value={data.label}
        onChange={(label) => onUpdate({ label })}
      />
      <label className="field">
        <span>Action</span>
        <SelectControl
          value={data.action}
          onChange={(e) =>
            onUpdate({ action: e.target.value as GitHostNodeData["action"] })
          }
        >
          <option value="pr">Create pull request</option>
          <option value="issue">Open issue</option>
        </SelectControl>
      </label>
      <label className="field">
        <span>Repository</span>
        <div className="field-row">
          <input
            type="text"
            list={`${suggestionsId}-repos`}
            style={{ flex: 1, minWidth: 0 }}
            value={repo}
            placeholder={
              gh?.current || "owner/name (empty uses the working directory)"
            }
            onChange={(e) => onUpdate({ repo: e.target.value })}
          />
          <datalist id={`${suggestionsId}-repos`}>
            {gh?.repos.map((name) => (
              <option key={name} value={name} />
            ))}
          </datalist>
          <button
            type="button"
            className="ghost"
            disabled={!gh}
            onClick={() => setReloadKey((key) => key + 1)}
          >
            {gh ? "Refresh" : "Loading…"}
          </button>
        </div>
      </label>
      {gh?.error ? <p className="hint">{gh.error}</p> : null}
      {gh && !gh.error && !repo.trim() && !gh.current ? (
        <p className="hint">
          No GitHub checkout in the working directory. Enter a repository above.
        </p>
      ) : null}
      <label className="field">
        <span>Title</span>
        <input
          type="text"
          value={data.title}
          placeholder="Required — supports {{output}}"
          onChange={(e) => onUpdate({ title: e.target.value })}
        />
      </label>
      <label className="field">
        <span>Body</span>
        <textarea
          rows={6}
          value={data.body}
          onChange={(e) => onUpdate({ body: e.target.value })}
        />
      </label>
      {isPr ? (
        <>
          <label className="field">
            <span>Base branch</span>
            <input
              type="text"
              list={`${suggestionsId}-branches`}
              value={data.base}
              placeholder="Repository default if empty"
              onChange={(e) => onUpdate({ base: e.target.value })}
            />
            <datalist id={`${suggestionsId}-branches`}>
              {currentBranches?.names.map((name) => (
                <option key={name} value={name} />
              ))}
            </datalist>
          </label>
          {currentBranches?.error ? (
            <p className="hint">{currentBranches.error}</p>
          ) : null}
          <label className="field">
            <span>Head branch</span>
            <input
              type="text"
              value={data.head ?? ""}
              placeholder={
                cwd?.trim()
                  ? "Current branch of the working directory"
                  : "Required without a workflow working directory"
              }
              onChange={(e) => onUpdate({ head: e.target.value })}
            />
          </label>
          <p className="hint">Push your branch before creating a pull request.</p>
          <label className="field checkbox-field">
            <input
              type="checkbox"
              checked={data.draft}
              onChange={(e) => onUpdate({ draft: e.target.checked })}
            />
            <span>Draft PR</span>
          </label>
        </>
      ) : null}
      <p className="hint">
        Requires the GitHub CLI (<code>gh</code>) authenticated in this
        environment. Title and body support <code>{"{{output}}"}</code>.
      </p>
    </>
  );
}

/** Live confirmation that `{{ask:…}}` placeholders were typed correctly. */
export function AskPreview({ text }: { text: string }) {
  const names = askNames(text);
  if (names.length === 0) return null;
  return <p className="hint">Asks on run: {names.join(", ")}</p>;
}
