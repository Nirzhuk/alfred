import { useEffect, useMemo, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Icon } from "../../../../components/icon";
import { SearchInput } from "../../../../components/search-input";
import { BuilderSetupPanel } from "../../../builder/components/builder-setup-panel";
import {
  listAlfredSkills,
  syncAlfredSkills,
  type AlfredSkill,
  type SkillSyncResult,
} from "../../../builder/api";
import { useBuilderStore } from "../../../builder/store";
import { useWorkflowStore } from "../../../workflow/store";
import "./mcp-skills-settings.css";

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function syncSummary(result: SkillSyncResult): string {
  const parts: string[] = [];
  if (result.linked) parts.push(`${result.linked} linked`);
  if (result.copied) parts.push(`${result.copied} copied`);
  if (result.removed) parts.push(`${result.removed} removed`);
  if (result.skipped.length) parts.push(`${result.skipped.length} skipped`);
  return parts.length ? parts.join(" · ") : "No shared skills linked.";
}

/**
 * Global MCP and skill visibility. MCP servers and the Claude Code bridge are
 * per-build settings; skills come from the user's home and the chosen project,
 * so the dev and stable builds see the same skill folders.
 */
export function McpSkillsSettings() {
  const config = useBuilderStore((state) => state.config);
  const info = useBuilderStore((state) => state.info);
  const configError = useBuilderStore((state) => state.configError);
  const saveConfig = useBuilderStore((state) => state.saveConfig);
  const init = useBuilderStore((state) => state.init);

  const activeWorkflowId = useWorkflowStore((state) => state.activeWorkflowId);
  const workflows = useWorkflowStore((state) => state.workflows);
  const activeProject = useMemo(() => {
    if (!activeWorkflowId) return null;
    const directory = workflows
      .find((workflow) => workflow.id === activeWorkflowId)
      ?.workingDirectory?.trim();
    return directory || null;
  }, [activeWorkflowId, workflows]);

  const [loaded, setLoaded] = useState(false);
  const [alfredSkills, setAlfredSkills] = useState<AlfredSkill[] | null>(null);
  const [skillsError, setSkillsError] = useState<string | null>(null);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [skillsRefresh, setSkillsRefresh] = useState(0);
  const [skillQuery, setSkillQuery] = useState("");
  const [sync, setSync] = useState<{
    busy?: boolean;
    result?: SkillSyncResult;
    error?: string;
  }>({});

  const projectRoot = activeProject;
  const visibleSkills = useMemo(() => {
    const query = skillQuery.trim().toLowerCase();
    if (!alfredSkills) return [];
    if (!query) return alfredSkills;
    return alfredSkills.filter((skill) =>
      `${skill.name} ${skill.path}`.toLowerCase().includes(query),
    );
  }, [alfredSkills, skillQuery]);

  useEffect(() => {
    let cancelled = false;
    void init().finally(() => {
      if (!cancelled) setLoaded(true);
    });
    return () => {
      cancelled = true;
    };
  }, [init]);

  useEffect(() => {
    let cancelled = false;
    setSkillsLoading(true);
    void listAlfredSkills()
      .then((next) => {
        if (cancelled) return;
        setAlfredSkills(next);
        setSkillsError(null);
      })
      .catch((error) => {
        if (cancelled) return;
        setAlfredSkills([]);
        setSkillsError(String(error));
      })
      .finally(() => {
        if (!cancelled) setSkillsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [skillsRefresh]);

  const refreshAll = () => {
    void init();
    setSkillsRefresh((value) => value + 1);
  };

  const syncSkills = async () => {
    setSync({ busy: true });
    try {
      const result = await syncAlfredSkills();
      setSync({ result });
      setSkillsRefresh((value) => value + 1);
    } catch (error) {
      setSync({ error: String(error) });
    }
  };

  const buildName =
    info?.externalMcp?.name ??
    (info?.build === "development"
      ? "alfred-dev"
      : info?.build === "production"
        ? "alfred"
        : null);
  const buildLabel =
    info?.build === "development"
      ? "Development build"
      : info?.build === "production"
        ? "Stable build"
        : "Build";
  const linkedCount =
    alfredSkills?.filter((skill) => skill.linked).length ?? 0;

  return (
    <>
      <section className="settings-section" aria-labelledby="mcp-build-heading">
        <div className="settings-section-heading">
          <div>
            <h2 id="mcp-build-heading">Build</h2>
          </div>
          <button type="button" className="ghost" onClick={refreshAll}>
            Refresh
          </button>
        </div>
        <div className="settings-card">
          <div className="settings-row settings-row-control">
            <div className="mcp-build-identity">
              <p className="settings-label">{buildLabel}</p>
              <p
                className="settings-value mcp-skills-path"
                title={info?.dataDir ?? undefined}
              >
                {info?.dataDir ? (
                  <code className="mcp-skills-root">{info.dataDir}</code>
                ) : (
                  "Checking…"
                )}
              </p>
            </div>
            <span className="mcp-build-badge">{buildName ?? "…"}</span>
          </div>
          <div className="settings-row settings-row-control">
            <p className="settings-label">Alfred API</p>
            <div
              className="mcp-build-status"
              title={info?.tools.map((tool) => tool.name).join("\n")}
            >
              <span className="settings-value">
                {info
                  ? info.mcpReady
                    ? `Ready · ${plural(info.tools.length, "tool")}`
                    : "Starting…"
                  : "Checking…"}
              </span>
              <span
                className={`mcp-build-dot${info?.mcpReady ? " is-on" : ""}`}
                aria-hidden
              />
            </div>
          </div>
        </div>
      </section>

      <section className="settings-section" aria-labelledby="mcp-servers-heading">
        <div className="settings-section-heading">
          <div>
            <h2 id="mcp-servers-heading">Speak with Alfred</h2>
            <p className="settings-section-copy">
              MCP tools for the builder agent.
            </p>
          </div>
        </div>
        <div className="settings-card mcp-skills-builder">
          {loaded ? (
            <BuilderSetupPanel
              sections={["mcp", "connect"]}
              info={info}
              agent={undefined}
              config={config}
              error={configError}
              onSave={saveConfig}
              suggestedPath={projectRoot}
            />
          ) : (
            <p className="mcp-skills-loading">Loading MCP configuration…</p>
          )}
        </div>
      </section>

      <section className="settings-section" aria-labelledby="mcp-skills-heading">
        <div className="settings-section-heading">
          <div>
            <h2 id="mcp-skills-heading">Skills</h2>
            <p className="settings-section-copy">
              Our shared <code>.agents</code> skills.
            </p>
          </div>
          <div className="mcp-skills-heading-actions">
            <button
              type="button"
              className="ghost"
              title="Sync shared skills to Claude Code"
              aria-label="Sync shared skills to Claude Code"
              disabled={sync.busy}
              onClick={() => void syncSkills()}
            >
              {sync.busy ? "Syncing…" : "Sync"}
            </button>
            <button
              type="button"
              className="ghost"
              disabled={skillsLoading}
              onClick={() => setSkillsRefresh((value) => value + 1)}
            >
              {skillsLoading ? "Reading…" : "Refresh"}
            </button>
          </div>
        </div>
        <div className="settings-card">
          <div className="settings-row settings-row-control">
            <div>
              <p className="settings-label">Alfred skills</p>
              <p className="settings-value">
                {alfredSkills === null
                  ? "Reading…"
                  : `${plural(alfredSkills.length, "skill")} · ${linkedCount} in Claude Code`}
              </p>
            </div>
          </div>

          <div className="mcp-skills-filter">
            <SearchInput
              aria-label="Filter skills"
              value={skillQuery}
              placeholder="Filter skills…"
              onChange={(event) => setSkillQuery(event.currentTarget.value)}
              onClear={() => setSkillQuery("")}
            />
          </div>

          {sync.result ? (
            <p className="mcp-skills-notice" role="status">
              {syncSummary(sync.result)}
            </p>
          ) : null}
          {sync.result && sync.result.skipped.length > 0 ? (
            <details className="mcp-skill-skipped">
              <summary>Skipped</summary>
              <ul className="mcp-skill-sync-list">
                {sync.result.skipped.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </details>
          ) : null}
          {sync.error ? (
            <p className="mcp-skills-error" role="alert">
              <Icon name="warning-circle" size={16} />
              {sync.error}
            </p>
          ) : null}
          {skillsError ? (
            <p className="mcp-skills-error" role="alert">
              <Icon name="warning-circle" size={16} />
              {skillsError}
            </p>
          ) : null}

          {alfredSkills === null ? null : alfredSkills.length === 0 ? (
            skillsError ? null : (
              <p className="mcp-skills-empty">No shared skills.</p>
            )
          ) : visibleSkills.length === 0 ? (
            <p className="mcp-skills-empty">
              No skills match “{skillQuery.trim()}”.
            </p>
          ) : (
            <ul className="mcp-alfred-skill-list">
              {visibleSkills.map((skill) => (
                <li
                  key={skill.path}
                  className="mcp-skill-row"
                  title={skill.path}
                >
                  <code className="mcp-skill-name">/{skill.name}</code>
                  <span
                    className={`mcp-skill-state${skill.linked ? " is-on" : ""}`}
                  >
                    {skill.linked ? "In Claude Code" : "Not linked"}
                  </span>
                  <button
                    type="button"
                    className="ghost mcp-skill-reveal"
                    title="Show in folder"
                    aria-label={`Show ${skill.name} in folder`}
                    onClick={() =>
                      void revealItemInDir(skill.path).catch((error) =>
                        setSkillsError(String(error)),
                      )
                    }
                  >
                    <Icon name="folder" size={14} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>
    </>
  );
}
