export type SettingsSectionId =
  | "general"
  | "quick-access"
  | "shortcuts"
  | "notifications"
  | "memory-review"
  | "license-billing"
  | "native-agents"
  | "connected-apps"
  | "sharing"
  | "data";

export const SETTINGS_SECTION_LABELS: Record<SettingsSectionId, string> = {
  general: "General",
  "quick-access": "Quick Access",
  shortcuts: "Keyboard shortcuts",
  notifications: "Notifications",
  "memory-review": "Memory review",
  "license-billing": "License & Billing",
  "native-agents": "Native Agents",
  "connected-apps": "Connected apps",
  sharing: "Shared workflows",
  data: "Data & storage",
};
