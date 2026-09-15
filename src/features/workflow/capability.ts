import {
  appBuildKind,
  resolveCapability,
  useLicenseStore,
  type Capability,
  type CapabilityDecision,
  type EntitlementInput,
  type LicenseStatus,
} from "../licensing";
import { holdsFreeScheduleSlot } from "./schedule-slot";

/**
 * The workflow feature's one door into the entitlement resolver (Plan 008
 * Step 4). Schedules and triggers are the two pro capabilities, and every
 * authoring surface asks here instead of re-deriving entitlement from
 * `LicenseState`.
 */

function inputFor(status: LicenseStatus | null): EntitlementInput {
  return {
    buildKind: appBuildKind,
    // Before the local status has been read the licence standing is unknown.
    // Unknown never locks — `notConfigured` resolves to available — so no
    // licensed user is flashed a padlock while the status loads. On a source
    // build the resolver answers available regardless of what lingers here.
    licenseState: status?.state ?? "notConfigured",
    inWindow: status ? status.inUpdateWindow : true,
  };
}

/**
 * Whether `capability` may be authored right now in this app. Reads the local
 * license store; the build kind comes from compile time and can never be
 * overridden at runtime.
 */
export function useWorkflowCapability(capability: Capability): CapabilityDecision {
  const status = useLicenseStore((state) => state.status);
  const hasLoaded = useLicenseStore((state) => state.hasLoaded);
  return resolveCapability(inputFor(hasLoaded ? status : null), capability);
}

/**
 * Schedule authoring gate with the one-free-schedule rule. A locked build
 * still edits the slot holder (oldest schedule, or a first creation when
 * nothing is saved); every other workflow stays locked delete-only.
 */
export function useScheduleGate(
  workflowId: string,
  schedules: { workflowId: string; createdAt?: string | null }[],
): CapabilityDecision {
  const base = useWorkflowCapability("schedules");
  if (base.available) return base;
  if (holdsFreeScheduleSlot(workflowId, schedules)) {
    return { available: true, reason: null };
  }
  return base;
}
