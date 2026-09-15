import { useMemo, useState } from "react";
import { SearchInput } from "../../../../components/search-input";
import type { Skill } from "../../types";
import { AgentMark } from "../../../../components/agent-mark";

type Props = {
  skills: Skill[];
  selectedNames: string[];
  onChange: (names: string[]) => void;
};

function shortDescription(text: string | undefined): string {
  const raw = (text ?? "").replace(/\s+/g, " ").trim();
  if (!raw) return "";
  const sentence = raw.split(/(?<=[.!?])\s+/)[0] ?? raw;
  if (sentence.length <= 72) return sentence;
  return `${sentence.slice(0, 71).trimEnd()}…`;
}

export function SkillPicker({ skills, selectedNames, onChange }: Props) {
  const [query, setQuery] = useState("");
  const selected = useMemo(() => new Set(selectedNames), [selectedNames]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = !q
      ? skills
      : skills.filter((skill) => {
          const hay = `${skill.name} ${skill.description}`.toLowerCase();
          return hay.includes(q);
        });
    // Selected first, then A–Z
    return [...list].sort((a, b) => {
      const aOn = selected.has(a.name) ? 0 : 1;
      const bOn = selected.has(b.name) ? 0 : 1;
      if (aOn !== bOn) return aOn - bOn;
      return a.name.localeCompare(b.name);
    });
  }, [skills, query, selected]);

  const toggle = (name: string) => {
    if (selected.has(name)) {
      onChange(selectedNames.filter((s) => s !== name));
      return;
    }
    onChange([...selectedNames, name]);
  };

  if (skills.length === 0) {
    return (
      <p className="muted skill-picker-empty">
        No skills found for this agent. Add a <code>SKILL.md</code> to its
        project or user skills directory.
      </p>
    );
  }

  return (
    <div className="skill-picker">
      {selectedNames.length > 0 ? (
        <div className="skill-picker-selected">
          <div className="skill-picker-chips">
            {selectedNames.map((name) => (
              <button
                key={name}
                type="button"
                className="skill-chip"
                title={`Remove /${name}`}
                onClick={() => toggle(name)}
              >
                <span>/{name}</span>
                <span className="skill-chip-x" aria-hidden>
                  ×
                </span>
              </button>
            ))}
          </div>
          <button
            type="button"
            className="ghost skill-picker-clear"
            onClick={() => onChange([])}
          >
            Clear
          </button>
        </div>
      ) : null}

      <div className="skill-picker-toolbar">
        <SearchInput
          aria-label="Search skills"
          containerClassName="skill-picker-search"
          value={query}
          placeholder="Search skills…"
          onChange={(e) => setQuery(e.target.value)}
          onClear={() => setQuery("")}
        />
        <span className="skill-picker-count muted">
          {selectedNames.length}/{skills.length}
        </span>
      </div>

      <ul className="skill-picker-list" role="listbox" aria-label="Skills">
        {filtered.length === 0 ? (
          <li className="skill-picker-empty muted">No matches.</li>
        ) : (
          filtered.map((skill) => {
            const on = selected.has(skill.name);
            const blurb = shortDescription(skill.description);
            return (
              <li key={`${skill.source}:${skill.path}`}>
                <button
                  type="button"
                  role="option"
                  aria-selected={on}
                  className={["skill-picker-row", on ? "is-selected" : ""]
                    .filter(Boolean)
                    .join(" ")}
                  onClick={() => toggle(skill.name)}
                  title={skill.description || undefined}
                >
                  <span
                    className={["skill-check", on ? "is-on" : ""]
                      .filter(Boolean)
                      .join(" ")}
                    aria-hidden
                  >
                    {on ? "✓" : ""}
                  </span>
                  <span className="skill-picker-copy">
                    <span className="skill-picker-name">/{skill.name}</span>
                    {blurb ? (
                      <span className="skill-picker-blurb">{blurb}</span>
                    ) : null}
                  </span>
                  <span className="skill-origin">
                    {skill.sourceAgent ? (
                      <AgentMark provider={skill.sourceAgent} size={13} />
                    ) : null}
                    <span className="skill-source">
                      {skill.source === "project" ? "Project" : "User"}
                    </span>
                  </span>
                </button>
              </li>
            );
          })
        )}
      </ul>
    </div>
  );
}
