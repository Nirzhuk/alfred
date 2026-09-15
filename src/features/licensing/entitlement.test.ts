import { describe, expect, test } from "bun:test";
import {
  appBuildKind,
  PRO_CAPABILITIES,
  readBuildKind,
  readLicenseGatingDisabled,
  resolveCapability,
  resolveEntitlement,
  type BuildKind,
  type Capability,
  type EntitlementInput,
} from "./entitlement";
import type { LicenseState } from "./types";

const ALL_STATES: LicenseState[] = [
  "unlicensed",
  "active",
  "offlineGrace",
  "needsOnline",
  "expired",
  "revoked",
  "disabled",
  "deviceLimit",
  "secureStorageUnavailable",
  "notConfigured",
];

const ENTITLED_STATES: LicenseState[] = [
  "active",
  "offlineGrace",
  "needsOnline",
  "expired",
];

const LOCKED_WITHOUT_WINDOW: LicenseState[] = ["unlicensed", "notConfigured"];

function input(
  overrides: Partial<EntitlementInput> = {},
): EntitlementInput {
  return {
    buildKind: "distribution",
    licenseState: "unlicensed",
    inWindow: true,
    ...overrides,
  };
}

describe("build kind", () => {
  test("reads the baked Polar environment exactly like Rust's config does", () => {
    expect(readBuildKind("production")).toBe("distribution");
    expect(readBuildKind("sandbox")).toBe("distribution");
    // vite.config.ts bakes "" when the .env entry is absent: a source build.
    expect(readBuildKind("")).toBe("source");
    expect(readBuildKind(undefined)).toBe("source");
    expect(readBuildKind("   ")).toBe("source");
    expect(readBuildKind(null)).toBe("source");
  });
  test("the explicit disable flag forces unlocked even with Polar values", () => {
    expect(readLicenseGatingDisabled("1")).toBe(true);
    expect(readLicenseGatingDisabled("true")).toBe(true);
    expect(readLicenseGatingDisabled("")).toBe(false);
    expect(readLicenseGatingDisabled(undefined)).toBe(false);
    expect(readLicenseGatingDisabled("0")).toBe(false);
  });

  test("the running bundle resolves its kind once from the same env value", () => {
    // The default export mirrors public-links.ts: one module-level read of
    // import.meta.env.ALFRED_POLAR_ENVIRONMENT. Under bun test no value is
    // baked, so this process reads as a source build.
    expect(appBuildKind).toBe(readBuildKind(process.env.ALFRED_POLAR_ENVIRONMENT));
  });
});

describe("entitlement matrix", () => {
  /** Plan 008 Step 2 requires the matrix to have no untested cell: every
   * licence state, under both build kinds, with the window open and closed. */
  for (const buildKind of ["source", "distribution"] as const) {
    for (const licenseState of ALL_STATES) {
      for (const inWindow of [true, false]) {
        const cell = `${buildKind} / ${licenseState} / ${
          inWindow ? "in-window" : "out-of-window"
        }`;
        test(cell, () => {
          const decision = resolveEntitlement({ buildKind, licenseState, inWindow });

          if (buildKind === "source") {
            // A source build is never locked. Not by a missing key, not by a
            // revoked one, not by a closed window. This is the offer.
            expect(decision).toEqual({ available: true, reason: null });
            return;
          }

          if (licenseState === "notConfigured") {
            // Licensing is absent from this distribution build, so nothing
            // can be gated either.
            expect(decision).toEqual({ available: true, reason: null });
            return;
          }

          if (!ENTITLED_STATES.includes(licenseState)) {
            // No purchase standing on this device: locked, whatever the
            // window says.
            expect(decision.available).toBe(false);
            expect(decision.reason).not.toBeNull();
            expect(decision.reason).not.toBe("outOfWindow");
            return;
          }

          if (!inWindow) {
            // Entitled but this build is newer than the paid window.
            expect(decision).toEqual({
              available: false,
              reason: "outOfWindow",
            });
            return;
          }

          expect(decision).toEqual({ available: true, reason: null });
        });
      }
    }
  }

  test("an expired-but-entitled licence keeps capabilities on its own build", () => {
    // The critical assertion: expired means "update window closed", and on
    // the build the customer already runs, that window was open at release.
    // The capability never disappears from under an existing install.
    expect(
      resolveEntitlement(input({ licenseState: "expired", inWindow: true })),
    ).toEqual({ available: true, reason: null });
  });

  test("a newer build locks pro features until renewal, without shaming words", () => {
    expect(
      resolveEntitlement(input({ licenseState: "expired", inWindow: false })),
    ).toEqual({ available: false, reason: "outOfWindow" });
    expect(
      resolveEntitlement(input({ licenseState: "active", inWindow: false })),
    ).toEqual({ available: false, reason: "outOfWindow" });
  });

  test("revocation and disablement end entitlement immediately, on any window", () => {
    for (const state of ["revoked", "disabled"] as const) {
      for (const inWindow of [true, false]) {
        const decision = resolveEntitlement(
          input({ licenseState: state, inWindow }),
        );
        expect(decision.available).toBe(false);
      }
    }
  });

  test("offline grace stays unlocked while the window holds", () => {
    expect(
      resolveEntitlement(input({ licenseState: "offlineGrace" })),
    ).toEqual({ available: true, reason: null });
  });

  test("unreadable secure storage fails closed but names itself", () => {
    // Alfred cannot prove the purchase it may still hold, so it does not
    // pretend; the reason lets UI copy point at the keychain, not at blame.
    const decision = resolveEntitlement(
      input({ licenseState: "secureStorageUnavailable" }),
    );
    expect(decision).toEqual({
      available: false,
      reason: "secureStorageUnavailable",
    });
  });

  test("a source build ignores even contradictory local state", () => {
    for (const licenseState of ALL_STATES) {
      for (const inWindow of [true, false]) {
        expect(
          resolveEntitlement({ buildKind: "source", licenseState, inWindow }),
        ).toEqual({ available: true, reason: null });
      }
    }
  });
});

describe("capability resolution", () => {
  test("the approved list is exactly the two supporter-license perks", () => {
    expect(PRO_CAPABILITIES).toEqual(["schedules", "triggers"]);
  });

  for (const capability of PRO_CAPABILITIES) {
    describe(`listed capability: ${capability}`, () => {
      test("a distribution build with no license locks it with a named reason", () => {
        expect(resolveCapability(input(), capability)).toEqual({
          available: false,
          reason: "noLicense",
        });
      });

      test("a source build never locks it, whatever state lingers locally", () => {
        for (const licenseState of ALL_STATES) {
          expect(
            resolveCapability(
              input({ buildKind: "source", licenseState }),
              capability,
            ),
          ).toEqual({ available: true, reason: null });
        }
      });

      test("every entitled licence standing unlocks it", () => {
        for (const licenseState of ENTITLED_STATES) {
          expect(
            resolveCapability(input({ licenseState }), capability),
          ).toEqual({ available: true, reason: null });
        }
      });

      test("an unentitled distribution state locks it without dropping the perk", () => {
        expect(
          resolveCapability(
            input({ licenseState: "unlicensed", inWindow: false }),
            capability,
          ).reason,
        ).toBe("noLicense");
        expect(
          resolveCapability(
            input({ licenseState: "expired", inWindow: false }),
            capability,
          ).reason,
        ).toBe("outOfWindow");
      });
    });
  }

  test("a capability off the list stays free in every build and state", () => {
    // Cast stand-ins prove the free-by-default contract: names not in
    // PRO_CAPABILITIES must never lock, whatever the licence says.
    for (const probe of ["workflowBatchRun", "anythingAtAll"] as const) {
      const capability = probe as unknown as Capability;
      expect(resolveCapability(input(), capability)).toEqual({
        available: true,
        reason: null,
      });
      expect(
        resolveCapability(
          input({ buildKind: "distribution", licenseState: "unlicensed", inWindow: false }),
          capability,
        ),
      ).toEqual({ available: true, reason: null });
    }
  });

  test("locked states always carry a named reason a component can render", () => {
    for (const state of ALL_STATES.filter(
      (candidate) =>
        !ENTITLED_STATES.includes(candidate) &&
        !LOCKED_WITHOUT_WINDOW.includes(candidate),
    )) {
      const decision = resolveEntitlement(input({ licenseState: state }));
      expect(typeof decision.reason).toBe("string");
    }
  });
});
