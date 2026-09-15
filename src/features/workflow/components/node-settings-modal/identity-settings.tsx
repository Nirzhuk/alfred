import type { IdentityNodeData } from "../../types";

export function IdentitySettings({
  data,
  onUpdate,
}: {
  data: IdentityNodeData;
  onUpdate: (patch: Partial<IdentityNodeData>) => void;
}) {
  return (
    <>
      <p className="hint">
        Connect this identity to an agent, directly or through other steps. Its
        instructions apply to every connected downstream agent on each run.
      </p>
      <label className="field">
        <span>Label</span>
        <input
          type="text"
          value={data.label}
          onChange={(event) => onUpdate({ label: event.target.value })}
        />
      </label>
      <label className="field">
        <span>Role</span>
        <input
          type="text"
          value={data.role}
          placeholder="e.g. Senior editor for our product blog"
          onChange={(event) => onUpdate({ role: event.target.value })}
        />
      </label>
      <label className="field">
        <span>Personality</span>
        <textarea
          rows={3}
          value={data.personality}
          placeholder="e.g. Direct, curious, and warm. Prefer plain language."
          onChange={(event) => onUpdate({ personality: event.target.value })}
        />
      </label>
      <label className="field">
        <span>Standing instructions</span>
        <textarea
          rows={7}
          value={data.instructions}
          placeholder="What should this agent always do or keep in mind?"
          onChange={(event) => onUpdate({ instructions: event.target.value })}
        />
      </label>
    </>
  );
}
