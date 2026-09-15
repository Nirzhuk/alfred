/**
 * Free-tier rule for cron schedules on distribution builds without a license.
 *
 * One schedule stays fully editable; anything beyond it is delete-only until
 * the user removes the extras or buys a license. The free slot belongs to the
 * oldest schedule (by `createdAt`), so deleting extras never steals the slot
 * from the schedule the user kept first.
 */

export const FREE_SCHEDULE_COUNT = 1;

type SlotItem = {
  workflowId: string;
  createdAt?: string | null;
};

/** Oldest schedule's workflow id, or null when there is nothing saved. */
export function freeScheduleWorkflowId<T extends SlotItem>(items: T[]): string | null {
  if (items.length === 0) return null;
  let oldest = items[0];
  for (const item of items) {
    if (!item.createdAt || !oldest.createdAt) continue;
    if (item.createdAt < oldest.createdAt) oldest = item;
  }
  return oldest.workflowId;
}

/**
 * Whether `workflowId` holds the free slot right now.
 * - No saved schedules: a new schedule would be the first, so it is free.
 * - One saved schedule: only that workflow is free; a second creation locks.
 * - Several: only the oldest is free.
 */
export function holdsFreeScheduleSlot<T extends SlotItem>(
  workflowId: string,
  items: T[],
): boolean {
  if (items.length === 0) return true;
  if (items.length === 1) return items[0].workflowId === workflowId;
  return freeScheduleWorkflowId(items) === workflowId;
}

/**
 * Workflows holding schedules beyond the free slot. Empty when within the
 * free tier — the caller can skip enforcement entirely.
 */
export function lockedExtraWorkflowIds<T extends SlotItem>(items: T[]): string[] {
  if (items.length <= FREE_SCHEDULE_COUNT) return [];
  const freeId = freeScheduleWorkflowId(items);
  return items
    .filter((item) => item.workflowId !== freeId)
    .map((item) => item.workflowId);
}
