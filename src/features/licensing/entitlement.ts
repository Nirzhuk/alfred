import type { LicenseState } from "./types";

/**
 * The one entitlement authority for pro capabilities (Plan 008). Components
 * ask this module whether a capability is available; they never re-derive
 * entitlement from `LicenseState` themselves.
 */

/** How this binary was produced. Plan 008 forbids a second notion of build
 * identity: a distribution build has the Polar configuration baked in, a
 * source build has none, and that is the same compile-time fact Rust already
 * resolves to `notConfigured` from the `ALFRED_POLAR_*` values. */
export type BuildKind = "distribution" | "source";

/** Maps the build-time Polar environment onto a build kind. An unset or empty
 * value means no Polar configuration reached the build: a source build, which
 * is fully functional by design and must never lock anything. The value comes
 * from `import.meta.env.ALFRED_POLAR_ENVIRONMENT`, baked in by
 * `vite.config.ts` from the same `.env` entry `build.rs` reads. */
export function readBuildKind(
  polarEnvironment: string | undefined | null,
): BuildKind {
  return polarEnvironment && polarEnvironment.trim() !== ""
    ? "distribution"
    : "source";
}

export function readLicenseGatingDisabled(
  raw: string | undefined | null,
): boolean {
  if (!raw) return false;
  const normalized = raw.trim().toLowerCase();
  return normalized === "1" || normalized === "true" || normalized === "yes";
}

const buildEnv = import.meta.env as {
  readonly ALFRED_POLAR_ENVIRONMENT?: string;
  readonly ALFRED_DISABLE_LICENSE_GATING?: string;
};

/**
 * Build-time kill-switch for self-compiled builds without Polar. Set
 * `ALFRED_DISABLE_LICENSE_GATING=1` in `.env` to force every capability
 * available, even when Polar values are present. Baked in by `vite.config.ts`
 * from the same `.env` entry `build.rs` reads. Empty by default: official
 * builds gate normally.
 */
export const appLicenseGatingDisabled: boolean = readLicenseGatingDisabled(
  buildEnv.ALFRED_DISABLE_LICENSE_GATING,
);

export const appBuildKind: BuildKind = appLicenseGatingDisabled
  ? "source"
  : readBuildKind(buildEnv.ALFRED_POLAR_ENVIRONMENT);

/**
 * The named pro capabilities a distribution build may lock. Owner-approved
 * for the paid-licensing program (Plan 008 Step 1): the two automation perks
 * of the one-time supporter license — cron schedules and event triggers
 * (file watch, webhook, connected app). Manual runs stay free in every
 * build, and both perks are permanent once purchased: keys carry no expiry,
 * so the update-window machinery stays idle for them.
 *
 * Guidance that survives any list: never gate data access, export, or history;
 * keep the free tier genuinely useful; prefer fewer, clearer capabilities.
 */
export const PRO_CAPABILITIES = ["schedules", "triggers"] as const;

/** A named, enumerated capability. Never a free-form string, never a licence
 * state. */
export type Capability = (typeof PRO_CAPABILITIES)[number];

export type EntitlementInput = {
  buildKind: BuildKind;
  licenseState: LicenseState;
  /** Whether THIS build was released inside the license's update window — the
   * rule `ALFRED_RELEASE_DATE <= licenseUpdateDeadline`. This is
   * `LicenseStatus.inUpdateWindow`; Rust owns the date comparison and the UI
   * never repeats it. */
  inWindow: boolean;
};

/** Why a capability is locked on this build. Named values, so callers can make
 * their own honest copy without matching prose. */
export type CapabilityLockReason =
  | "noLicense"
  | "outOfWindow"
  | "revoked"
  | "disabled"
  | "deviceLimit"
  | "secureStorageUnavailable";

export type CapabilityDecision = {
  available: boolean;
  /** Why the capability is locked; always null while available. */
  reason: CapabilityLockReason | null;
};

/** The named reason each unentitled distribution state locks with. A state
 * with no entry here still proves a completed purchase — mirrors
 * `LicenseStatus::is_entitled` in `src-tauri/src/licensing/models.rs`:
 * `offlineGrace` and `needsOnline` are connectivity problems, not verdicts;
 * `expired` means the update window closed, not that access ended;
 * `revoked` and `disabled` are the verdicts that end entitlement and must
 * never be folded in with the others. */
const LOCKED_STATE_REASONS: Partial<
  Record<LicenseState, CapabilityLockReason>
> = {
  unlicensed: "noLicense",
  revoked: "revoked",
  disabled: "disabled",
  deviceLimit: "deviceLimit",
  secureStorageUnavailable: "secureStorageUnavailable",
};

/**
 * Whether the license standing of this build permits pro use at all. A source
 * build answers yes unconditionally — it is a first-class product, and a gate
 * there could only be enforcement of a GPL binary, which Plan 008 forbids. A
 * distribution build follows the license: entitled states pass, subject to
 * the update window; every other state names its reason.
 */
export function resolveEntitlement(
  input: EntitlementInput,
): CapabilityDecision {
  // Explicit self-compile opt-out: never lock, even if Polar values leaked in.
  if (appLicenseGatingDisabled) return { available: true, reason: null };
  // A source build is never locked, whatever state lingers locally.
  if (input.buildKind === "source") return { available: true, reason: null };

  // Licensing is absent from this build, so nothing here can be gated either.
  if (input.licenseState === "notConfigured")
    return { available: true, reason: null };

  const reason = LOCKED_STATE_REASONS[input.licenseState];
  if (reason) return { available: false, reason };

  if (!input.inWindow) {
    return { available: false, reason: "outOfWindow" };
  }

  return { available: true, reason: null };
}

/**
 * THE seam components call. Answers "is this capability available right now?"
 * Anything not on the approved pro list is free and available in every build;
 * a listed capability additionally needs the license standing to permit pro
 * use.
 */
export function resolveCapability(
  input: EntitlementInput,
  capability: Capability,
): CapabilityDecision {
  if (!PRO_CAPABILITIES.includes(capability)) {
    return { available: true, reason: null };
  }
  return resolveEntitlement(input);
}
