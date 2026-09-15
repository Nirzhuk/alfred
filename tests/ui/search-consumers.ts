/** Every production SearchInput call must have an actual-screen browser scenario. */
export const searchConsumerSources = {
  library: "src/features/workflow/components/memories-inspector/memory-library-list.tsx",
  link: "src/features/workflow/components/memories-inspector/memories-inspector.tsx",
  "memory-node": "src/features/workflow/components/node-settings-modal/memory-node-settings.tsx",
  skills: "src/features/workflow/components/skill-picker/skill-picker.tsx",
  "action-options": "src/features/integrations/app-action-settings.tsx",
  "trigger-options": "src/features/workflow/components/triggers-modal/triggers-modal.tsx",
  settings: "src/features/settings/components/settings-sidebar/settings-sidebar.tsx",
  history: "src/features/workflow/components/history-page/history-page.tsx",
  activity: "src/features/workflow/components/run-activity-panel/run-activity-panel.tsx",
  resources: "src/features/workflow/components/workflow-resources-panel/workflow-resources-panel.tsx",
  "add-step": "src/features/workflow/components/add-step-menu-items.tsx",
} as const;
