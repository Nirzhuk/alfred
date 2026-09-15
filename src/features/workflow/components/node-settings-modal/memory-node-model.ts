import { isMemoryPromptEligible, withMemoryDefaults } from "../../memories";
import type { OutputMemory } from "../../types";

export function memoryPickerItems(
  memories: OutputMemory[],
  linkable: OutputMemory[],
  selectedIds: string[],
  now: Date | number = Date.now(),
) {
  const byId = new Map(linkable.map((memory) => [memory.id, memory]));
  for (const memory of memories) byId.set(memory.id, memory);
  const selected = [...new Set(selectedIds)].map((id) => ({
    id,
    memory: byId.get(id) ?? null,
  }));
  const selectedSet = new Set(selectedIds);
  const available = [...byId.values()].filter(
    (memory) =>
      !selectedSet.has(memory.id) &&
      isMemoryPromptEligible(withMemoryDefaults(memory), now),
  );
  return { selected, available };
}

export async function addMemorySelection(
  memoryId: string,
  options: {
    requiresLink: boolean;
    linkMemory: (id: string) => Promise<OutputMemory | null>;
    currentSelection: () => string[];
    isCurrent: () => boolean;
  },
): Promise<string[] | null> {
  if (options.requiresLink && !(await options.linkMemory(memoryId)))
    return null;
  if (!options.isCurrent()) return null;
  return [...new Set([...options.currentSelection(), memoryId])];
}
