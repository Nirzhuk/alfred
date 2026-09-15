export type SearchFixture = {
  scenarios: { id: string; label: string; selector: string }[];
  show: (id: string) => Promise<void>;
  setTheme: (theme: "light" | "dark") => void | Promise<void>;
  getSearch: () => HTMLInputElement;
  reference: () => HTMLInputElement;
  setReference?: (state: { value?: string; disabled?: boolean; readOnly?: boolean; width?: number }) => void | Promise<void>;
  unhandled: unknown[];
};

const settled = () =>
  new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
const inputProperties = [
  "backgroundColor",
  "backgroundImage",
  "color",
  "fontFamily",
  "fontSize",
  "fontWeight",
  "lineHeight",
  "letterSpacing",
  "borderTopWidth",
  "borderRightWidth",
  "borderBottomWidth",
  "borderLeftWidth",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "boxShadow",
  "opacity",
] as const;
const outerProperties = [
  "backgroundColor",
  "color",
  "borderRadius",
  "borderTopColor",
  "borderTopWidth",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "columnGap",
] as const;
const clearProperties = [
  "backgroundColor",
  "color",
  "width",
  "height",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "borderTopWidth",
  "boxShadow",
  "transform",
] as const;

function metrics(input: HTMLInputElement) {
  const wrapper = input.closest<HTMLElement>(".ui-search")!;
  const clear = wrapper.querySelector<HTMLButtonElement>(".ui-search-clear");
  const icon = wrapper.querySelector<SVGElement>(":scope > svg")!;
  const collect = (element: Element, properties: readonly string[]) => {
    const style = getComputedStyle(element);
    return Object.fromEntries(
      properties.map((property) => [
        property,
        style[property as keyof CSSStyleDeclaration],
      ]),
    );
  };
  return {
    input: collect(input, inputProperties),
    outer: collect(wrapper, outerProperties),
    clear: clear ? collect(clear, clearProperties) : null,
    height: wrapper.getBoundingClientRect().height,
    iconWidth: icon.getBoundingClientRect().width,
    iconHeight: icon.getBoundingClientRect().height,
    overflow: wrapper.scrollWidth > wrapper.clientWidth + 1,
  };
}

async function fill(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
  await settled();
}

export async function runSearchAudit(
  fixture: SearchFixture,
  options: { ids?: string[]; themes?: ("light" | "dark")[] } = {},
) {
  const failures: string[] = [];
  const samples: {
    scenario: string;
    theme: string;
    state: string;
    metrics: ReturnType<typeof metrics>;
  }[] = [];
  const compare = (
    prefix: string,
    actual: ReturnType<typeof metrics>,
    expected: ReturnType<typeof metrics>,
  ) => {
    for (const group of ["input", "outer", "clear"] as const) {
      const left = actual[group];
      const right = expected[group];
      if (Boolean(left) !== Boolean(right))
        failures.push(`${prefix}: ${group} presence differs`);
      if (!left || !right) continue;
      for (const property of Object.keys(right)) {
        if (left[property] !== right[property])
          failures.push(
            `${prefix}: ${group}.${property} = ${left[property]}, expected ${right[property]}`,
          );
      }
    }
    for (const property of ["height", "iconWidth", "iconHeight"] as const) {
      if (Math.abs(actual[property] - expected[property]) > 0.5)
        failures.push(
          `${prefix}: ${property} = ${actual[property]}, expected ${expected[property]}`,
        );
    }
    if (actual.overflow) failures.push(`${prefix}: horizontal overflow`);
    if (actual.input.backgroundColor !== "rgba(0, 0, 0, 0)")
      failures.push(`${prefix}: input must remain transparent`);
  };
  for (const theme of options.themes ?? ["light", "dark"]) {
    await fixture.setTheme(theme);
    for (const scenario of fixture.scenarios.filter(
      (item) => !options.ids || options.ids.includes(item.id),
    )) {
      await fixture.show(scenario.id);
      await settled();
      for (const state of ["empty", "query", "focused"] as const) {
        const reference = fixture.reference();
        await fill(reference, state === "empty" ? "" : "audit");
        if (state === "focused") reference.focus();
        else reference.blur();
        await settled();
        const expected = metrics(reference);
        const input = fixture.getSearch();
        await fill(input, state === "empty" ? "" : "audit");
        if (state === "focused") input.focus();
        else input.blur();
        await settled();
        const actual = metrics(fixture.getSearch());
        const prefix = `${theme}/${scenario.id}/${state}`;
        compare(prefix, actual, expected);
        samples.push({ scenario: scenario.id, theme, state, metrics: actual });
      }
      const input = fixture.getSearch();
      const clear = input
        .closest(".ui-search")
        ?.querySelector<HTMLButtonElement>(".ui-search-clear");
      if (!input.disabled && !input.readOnly) {
        if (!clear)
          failures.push(`${theme}/${scenario.id}: missing clear button`);
        else {
          clear.click();
          await settled();
          if (fixture.getSearch().value !== "")
            failures.push(`${theme}/${scenario.id}: clear did not empty value`);
          if (document.activeElement !== fixture.getSearch())
            failures.push(
              `${theme}/${scenario.id}: clear did not restore input focus`,
            );
        }
      }
    }
  }
  if (fixture.unhandled.length)
    failures.push(`Unhandled fixture calls: ${fixture.unhandled.length}`);
  return { passed: failures.length === 0, failures, samples };
}

export async function runSearchOrderAudit(fixture: SearchFixture) {
  const sheet = [
    ...document.querySelectorAll<HTMLStyleElement>("style[data-vite-dev-id]"),
  ].find((element) =>
    element.dataset.viteDevId?.endsWith("/search-input/search-input.css"),
  );
  if (!sheet?.parentNode)
    throw new Error(
      "Run the stylesheet-order audit through the Vite fixture page.",
    );
  const parent = sheet.parentNode;
  const next = sheet.nextSibling;
  try {
    parent.insertBefore(sheet, parent.firstChild);
    const sharedFirst = await runSearchAudit(fixture);
    parent.appendChild(sheet);
    const sharedLast = await runSearchAudit(fixture);
    return {
      passed: sharedFirst.passed && sharedLast.passed,
      sharedFirst,
      sharedLast,
    };
  } finally {
    parent.insertBefore(sheet, next?.parentNode === parent ? next : null);
  }
}

export async function proveModalRegressionIsCaught(
  fixture: SearchFixture,
  modalScenarioId: string,
) {
  const regression = document.createElement("style");
  regression.textContent =
    '.modal input:not([type="checkbox"]):not([type="radio"]) { background-color: var(--surface-raised); }';
  document.head.append(regression);
  try {
    const result = await runSearchAudit(fixture, {
      ids: [modalScenarioId],
      themes: ["dark"],
    });
    return {
      detected: result.failures.some((failure) =>
        failure.includes("input must remain transparent"),
      ),
      failures: result.failures,
    };
  } finally {
    regression.remove();
  }
}

export async function runSearchStateAudit(fixture: SearchFixture) {
  if (!fixture.setReference) throw new Error("Fixture must provide controlled reference states.");
  const failures: string[] = [];
  for (const theme of ["light", "dark"] as const) {
    await fixture.setTheme(theme);
    await fixture.setReference({ value: "audit", disabled: true, readOnly: false, width: 170 });
    await settled();
    let input = fixture.reference();
    let wrapper = input.closest<HTMLElement>(".ui-search")!;
    const clear = wrapper.querySelector<HTMLButtonElement>(".ui-search-clear");
    if (!input.disabled || !clear?.disabled) failures.push(`${theme}: disabled input and clear must both be disabled`);
    if (clear && getComputedStyle(clear).opacity !== "1") failures.push(`${theme}: disabled clear is dimmed twice (local opacity ${getComputedStyle(clear).opacity})`);
    if (clear) {
      clear.click(); await settled();
      if (fixture.reference().value !== "audit") failures.push(`${theme}: disabled clear changed query`);
    }
    if (wrapper.scrollWidth > wrapper.clientWidth + 1) failures.push(`${theme}: narrow disabled field overflows`);
    await fixture.setReference({ value: "audit", disabled: false, readOnly: true });
    await settled();
    input = fixture.reference();
    wrapper = input.closest<HTMLElement>(".ui-search")!;
    if (!input.readOnly || wrapper.querySelector(".ui-search-clear")) failures.push(`${theme}: read-only field must not offer clear`);
    await fixture.setReference({ value: "", disabled: false, readOnly: false, width: 320 });
    await settled();
  }
  return { passed: failures.length === 0, failures };
}
