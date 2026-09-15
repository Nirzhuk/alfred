export { createLicensingApi, licensingApi } from "./api";
export { openLatestDownload } from "./download-latest";
export {
  PRO_CAPABILITIES,
  appBuildKind,
  appLicenseGatingDisabled,
  readBuildKind,
  readLicenseGatingDisabled,
  resolveCapability,
  resolveEntitlement,
} from "./entitlement";
export type {
  BuildKind,
  Capability,
  CapabilityDecision,
  CapabilityLockReason,
  EntitlementInput,
} from "./entitlement";
export {
  UPDATE_WINDOW_NOTICE_BODY,
  UPDATE_WINDOW_NOTICE_KEY,
  UPDATE_WINDOW_NOTICE_TITLE,
  dismissUpdateWindowNotice,
  readUpdateWindowNoticeDismissed,
} from "./update-window-notice";
export { LicenseBadge, LicenseSettings } from "./components";
export {
  createPolarPublicLinks,
  parsePolarPublicLink,
  polarPublicLinks,
} from "./public-links";
export { createLicenseStore, useLicenseStore } from "./store";
export type {
  LicenseCommandError,
  LicenseProduct,
  LicenseState,
  LicenseStatus,
} from "./types";
