import { SearchInput } from "../../../../components/search-input";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { AgentMark } from "../../../../components/agent-mark";
import { Icon, type IconProps } from "../../../../components/icon";
import { getRunHistory, listRunHistory } from "../../api";
import { getWorkflowResources, type ResourceItem } from "../../resources";
import { useWorkflowStore } from "../../store";
import type { RunHistoryMemoryUse } from "../../types";
import "./workflow-resources-panel.css";

type Props = {
  workflowId: string;
  workflowName: string;
  onClose: () => void;
  onOpenMemories: (memoryId?: string) => void;
  onOpenRunHistory: (runId: string) => void;
};

function ResourceSection({
  title,
  icon,
  items,
  query,
  children,
  onOpen,
  selectedNodeId,
}: {
  title: string;
  icon: IconProps["name"];
  items: ResourceItem[];
  query: string;
  children?: ReactNode;
  onOpen: (item: ResourceItem) => void;
  selectedNodeId: string | null;
}) {
  const [expanded, setExpanded] = useState(false);
  useEffect(() => {
    if (query) setExpanded(true);
  }, [query]);
  const visibleItems = items.filter((item) =>
    item.searchText.toLowerCase().includes(query),
  );
  const showItems = expanded;
  if (query && !visibleItems.length) return null;

  return (
    <section className="resource-section" aria-label={title}>
      <button
        type="button"
        className="resource-section-toggle"
        aria-expanded={showItems}
        onClick={() => setExpanded(!expanded)}
      >
        <Icon name={icon} />
        <span>{title}</span>
        <span className="resource-count">{visibleItems.length}</span>
        <Icon
          name="caret-down"
          size={12}
          className={showItems ? "" : "is-collapsed"}
        />
      </button>
      {showItems && (visibleItems.length > 0 || children) ? (
        <div className="resource-section-content">
          {visibleItems.length ? (
            <ul className="resource-list">
              {visibleItems.map((item) => (
                <li key={item.id}>
                  <button
                    type="button"
                    className={`resource-item${selectedNodeId && item.nodeIds.includes(selectedNodeId) ? " is-selected" : ""}`}
                    onClick={() => onOpen(item)}
                    title={[item.title, item.description]
                      .filter(Boolean)
                      .join("\n")}
                  >
                    <span className="resource-item-icon">
                      {item.provider ? (
                        <AgentMark provider={item.provider} size={16} />
                      ) : (
                        <Icon name={item.icon ?? icon} />
                      )}
                    </span>
                    <span className="resource-item-copy">
                      <span className="resource-item-title">{item.title}</span>
                    </span>
                    {item.badge ? (
                      <span className="resource-badge">{item.badge}</span>
                    ) : null}
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          {children}
        </div>
      ) : null}
    </section>
  );
}

export function WorkflowResourcesPanel({
  workflowId,
  workflowName,
  onClose,
  onOpenMemories,
  onOpenRunHistory,
}: Props) {
  const nodes = useWorkflowStore((s) => s.nodes);
  const memories = useWorkflowStore((s) => s.memories);
  const skills = useWorkflowStore((s) => s.skills);
  const activeRun = useWorkflowStore((s) => s.activeRun);
  const stepStatuses = useWorkflowStore((s) => s.stepStatuses);
  const selectedNodeId = useWorkflowStore((s) => s.selectedNodeId);
  const openRunPanel = useWorkflowStore((s) => s.openRunPanel);
  const [query, setQuery] = useState("");
  const [skillTarget, setSkillTarget] = useState<ResourceItem | null>(null);
  const [runMemory, setRunMemory] = useState<{
    runId: string;
    uses: RunHistoryMemoryUse[];
  } | null>(null);
  const [historyError, setHistoryError] = useState(false);
  const activeRunId =
    activeRun?.workflowId === workflowId ? activeRun.id : undefined;
  const runStatus = activeRun?.status;
  const stepsKey = Object.entries(stepStatuses)
    .map(([id, status]) => `${id}:${status}`)
    .join("|");

  useEffect(() => {
    let cancelled = false;
    setHistoryError(false);
    const loadUsage = async () => {
      const runId =
        activeRunId ?? (await listRunHistory({ workflowId, limit: 1 }))[0]?.id;
      if (!runId || cancelled) return null;
      const detail = await getRunHistory(runId);
      return { runId, uses: detail?.memoryUses ?? [] };
    };
    void loadUsage()
      .then((result) => {
        if (!cancelled) setRunMemory(result);
      })
      .catch(() => {
        if (!cancelled) setHistoryError(true);
      });
    return () => {
      cancelled = true;
    };
  }, [workflowId, activeRunId, runStatus, stepsKey]);

  const runId = activeRunId ?? runMemory?.runId;

  const resources = useMemo(
    () =>
      getWorkflowResources({
        nodes,
        memories,
        skills,
        memoryUses: runMemory?.runId === runId ? runMemory?.uses : [],
      }),
    [nodes, memories, skills, runMemory, runId],
  );
  const search = query.trim().toLowerCase();
  const resultCount = Object.values(resources)
    .flat()
    .filter((item) => item.searchText.toLowerCase().includes(search)).length;
  const currentSkillTarget = resources.skills.find(
    (item) => item.id === skillTarget?.id,
  );

  const openNode = (nodeId: string) => {
    setSkillTarget(null);
    window.dispatchEvent(
      new CustomEvent("alfred:edit-node", { detail: { workflowId, nodeId } }),
    );
  };
  const openResource = (item: ResourceItem) => {
    if (item.memoryId) onOpenMemories(item.memoryId);
    else if (item.id.startsWith("memory:") && runId) onOpenRunHistory(runId);
    else if (item.nodeIds.length > 1) setSkillTarget(item);
    else if (item.nodeIds[0]) openNode(item.nodeIds[0]);
    else if (runId) onOpenRunHistory(runId);
  };
  const sectionProps = { query: search, onOpen: openResource, selectedNodeId };

  return (
    <aside
      id="workflow-resources"
      className="workflow-resources"
      aria-label="Workflow resources"
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        if (query) setQuery("");
        else onClose();
      }}
    >
      <header className="resources-header">
        <div>
          <h2>Resources</h2>
          <p title={workflowName}>{workflowName}</p>
        </div>
        <button
          type="button"
          className="ghost resources-close"
          onClick={onClose}
          aria-label="Close resources"
          title="Close resources"
        >
          <Icon name="sidebar-simple" />
        </button>
      </header>
      <SearchInput
        containerClassName="resources-search"
        clearLabel="Clear resource search"
        onClear={() => setQuery("")}
        type="search"
        autoFocus
        aria-label="Search workflow resources"
        placeholder="Find a resource…"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
      />
      <div className="resources-scroll">
        {currentSkillTarget ? (
          <div className="resource-skill-targets">
            <button
              type="button"
              className="resource-link"
              onClick={() => setSkillTarget(null)}
            >
              <Icon name="arrow-left" size={12} />
              All resources
            </button>
            <p className="resource-empty">
              /{currentSkillTarget.title.replace(/^\//, "")}
            </p>
            {nodes
              .filter((node) => currentSkillTarget.nodeIds.includes(node.id))
              .map((node) => (
                <button
                  key={node.id}
                  type="button"
                  className="resource-item"
                  onClick={() => openNode(node.id)}
                >
                  {node.data.label || "Agent"}
                  <Icon name="arrow-right" size={12} />
                </button>
              ))}
          </div>
        ) : null}
        {search && resultCount === 0 ? (
          <p className="resource-empty resources-no-results" role="status">
            No resources match "{query.trim()}".
          </p>
        ) : null}
        <ResourceSection
          {...sectionProps}
          title="Memories"
          icon="database"
          items={resources.memories}
        >
          <button
            type="button"
            className="resource-link"
            onClick={() => onOpenMemories()}
          >
            Manage memories
            <Icon name="arrow-right" size={12} />
          </button>
          {historyError ? (
            <p className="resource-empty" role="status">
              Run memory usage could not be loaded.
            </p>
          ) : null}
        </ResourceSection>
        <ResourceSection
          {...sectionProps}
          title="Skills"
          icon="code"
          items={resources.skills}
        />
        <ResourceSection
          {...sectionProps}
          title="Agents"
          icon="chat-text"
          items={resources.agents}
        >
          <button
            type="button"
            className="resource-link"
            disabled={!runId}
            onClick={() => {
              if (activeRunId) openRunPanel(null);
              else if (runId) onOpenRunHistory(runId);
            }}
          >
            Agent &amp; subagent activity
            <Icon name="arrow-right" size={12} />
          </button>
        </ResourceSection>
        <ResourceSection
          {...sectionProps}
          title="Tools & context"
          icon="terminal-window"
          items={resources.tools}
        />
      </div>
    </aside>
  );
}
