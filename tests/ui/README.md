# Search controls in their real consumers

Run the existing Vite development server, then open:

`http://localhost:1420/tests/ui/search-controls.html`

This entry point imports `App.css` and the real components. Its eleven scenarios cover the library and nested link modal, Memories node, canvas Add step menu, Settings sidebar, History page, Agent skill picker, Activity panel, Resources panel, and action/trigger resource selectors. Scenarios retain the containing modal, menu, sidebar, field, and panel classes that can affect the CSS cascade.

The fixture uses the official Tauri IPC mocks and in-memory workflow/integration stores. All supported read commands return fixture data. Unknown commands fail closed and are recorded in `searchFixture.unhandled`; nothing falls through to the native backend. It does not read or modify a user's database, credentials, or workflows.

Use the scenario menu for visual inspection. The browser console also exposes:

```js
await searchFixture.show('library');
searchFixture.setTheme('dark');
await runSearchAudit(); // all eleven consumers, both themes, empty/query/focus + clear/refocus
await runSearchStateAudit(); // disabled/readOnly and narrow reference controls
await runSearchOrderAudit(); // shared CSS moved first and last, restored afterward
await proveModalRegressionIsCaught(); // inject the original modal bug; must report detected:true
```

The negative test removes the injected rule in `finally`. A normal audit should pass after it. Browser-computed metrics compare actual consumers with a laid-out reference SearchInput and enforce transparency, geometry, typography, and overflow invariants. The reference is deliberately outside consumer wrappers so inherited CSS drift remains detectable.

Some browser automation connections time out while waiting for the full matrix. Start it without awaiting the console transport, then read the retained result:

```js
window.auditResult = null;
runSearchAudit().then(result => window.auditResult = result);
// Read window.auditResult after it finishes.
```

Typecheck both the fixture and assertion module with:

```sh
bunx tsc --noEmit -p tests/ui/tsconfig.json
```

This is a component/browser CSS audit with real consumer markup and mocked data. It does not replace native backend integration tests.

For a physical pressed-state check, call `searchFixture.armPressSample()`, then hold a populated search clear button for at least 150ms using the browser/device tooling. `searchFixture.pressSamples` records the real `:active` transform and background. It should report `transform: "none"` consistently across consumers. Synthetic `.click()` does not exercise CSS `:active`.

## Schedule surface fixtures

Two further entry points cover the schedule surfaces. Both are render-only or
IPC-mocked, and neither reads a user database:

- `http://localhost:1420/tests/ui/missed-schedules.html?theme=dark&scenario=normal`
  — the open-time missed-run prompt, one Run now / Skip pair per schedule
  (`normal`, `single`, `error` scenarios). Answers are recorded in
  `window.missedFixture.decisions`; no workflow runs.
- `http://localhost:1420/tests/ui/schedule-modal.html?theme=dark&policy=ask`
  — the real `ScheduleModal` over mocked IPC (`policy=none` renders a workflow
  with no saved schedule), covering the missed-run choice control inside the
  modal. Invocations are recorded in `window.scheduleFixture.calls`.
- `http://localhost:1420/tests/ui/schedule-canvas.html?scenario=chain&theme=dark`
  — the real `WorkflowCanvas` over mocked IPC, covering the schedule node and
  its generated start arrows. Scenarios: `chain`, `branches`, `empty`,
  `disabled`, `invalid`, `over-limit`; add `&license=unlicensed` for a
  distribution build whose only free schedule slot is taken. Inspect through
  `window.scheduleCanvasFixture` (`node()`, `arrows()`, `calls`, `layouts`,
  `store`, `errors`). Nothing runs and no workflow leaves the browser.
