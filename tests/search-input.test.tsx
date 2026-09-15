import { describe, expect, test } from "bun:test";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { searchConsumerSources } from "./ui/search-consumers";

const root = new URL("../", import.meta.url);

function rawSearchInputs(source: string): number[] {
  const file = ts.createSourceFile(
    "view.tsx",
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  const matches: number[] = [];
  const visit = (node: ts.Node) => {
    if (
      (ts.isJsxSelfClosingElement(node) || ts.isJsxOpeningElement(node)) &&
      node.tagName.getText(file) === "input"
    ) {
      const attributes = new Map<string, string>();
      for (const attribute of node.attributes.properties) {
        if (!ts.isJsxAttribute(attribute)) continue;
        const value = attribute.initializer;
        if (value && ts.isStringLiteral(value))
          attributes.set(attribute.name.getText(file), value.text);
        else if (
          value &&
          ts.isJsxExpression(value) &&
          value.expression &&
          (ts.isStringLiteral(value.expression) ||
            ts.isNoSubstitutionTemplateLiteral(value.expression))
        )
          attributes.set(attribute.name.getText(file), value.expression.text);
        else if (
          value &&
          ts.isJsxExpression(value) &&
          value.expression &&
          ts.isTemplateExpression(value.expression)
        ) {
          attributes.set(
            attribute.name.getText(file),
            [
              value.expression.head.text,
              ...value.expression.templateSpans.map(
                (span) => span.literal.text,
              ),
            ].join(" "),
          );
        }
      }
      if (
        attributes.get("type") === "search" ||
        /\b(search|find|filter)\b/i.test(
          [attributes.get("aria-label"), attributes.get("placeholder")].join(
            " ",
          ),
        )
      ) {
        matches.push(
          file.getLineAndCharacterOfPosition(node.getStart(file)).line + 1,
        );
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return matches;
}

describe("search component ownership", () => {
  test("every production search has an actual-screen browser scenario", async () => {
    const consumers: string[] = [];
    for await (const path of new Bun.Glob("src/**/*.tsx").scan({ cwd: fileURLToPath(root) })) {
      if (/\.(test|spec)\.tsx$/.test(path)) continue;
      const source = await Bun.file(new URL(path, root)).text();
      const file = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
      const imports = new Set<string>();
      for (const statement of file.statements) {
        if (!ts.isImportDeclaration(statement) || !ts.isStringLiteral(statement.moduleSpecifier) || !statement.moduleSpecifier.text.endsWith("/search-input")) continue;
        const bindings = statement.importClause?.namedBindings;
        if (bindings && ts.isNamedImports(bindings)) {
          for (const binding of bindings.elements) if ((binding.propertyName ?? binding.name).text === "SearchInput") imports.add(binding.name.text);
        }
      }
      const visit = (node: ts.Node) => {
        if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) && imports.has(node.tagName.getText(file))) consumers.push(path);
        ts.forEachChild(node, visit);
      };
      visit(file);
    }
    expect(consumers.sort()).toEqual(Object.values(searchConsumerSources).sort());
  });
  test("modal input defaults leave the shared search background alone", async () => {
    const css = await Bun.file(new URL("src/App.css", root)).text();
    const modalInputs = css.match(
      /\.modal input[^{}]*\{[^}]*background-color:\s*var\(--surface-raised\);[^}]*\}/g,
    );
    expect(modalInputs?.length).toBeGreaterThan(0);
    for (const rule of modalInputs ?? []) {
      expect(rule).toContain(":not(.ui-search-input)");
    }
  });
  test("detects raw text searches even after JSX event handlers", () => {
    expect(
      rawSearchInputs(
        '<input onChange={(event) => update(event)} type="text" aria-label="Search memories" />',
      ),
    ).toEqual([1]);
    expect(rawSearchInputs('<input type={"search"} />')).toEqual([1]);
    expect(rawSearchInputs('<input placeholder="Find a resource…" />')).toEqual(
      [1],
    );
    expect(
      rawSearchInputs("<input aria-label={`Search ${field.label} options`} />"),
    ).toEqual([1]);
    expect(rawSearchInputs("<input type={`search`} />")).toEqual([1]);
    expect(rawSearchInputs('<SearchInput aria-label="Search" />')).toEqual([]);
    expect(rawSearchInputs('<input aria-label="Memory title" />')).toEqual([]);
  });

  test("production screens use the shared component for every search field", async () => {
    const violations: string[] = [];
    for await (const path of new Bun.Glob("src/**/*.tsx").scan({
      cwd: fileURLToPath(root),
    })) {
      if (path.includes("/search-input/") || /\.(test|spec)\.tsx$/.test(path))
        continue;
      const source = await Bun.file(new URL(path, root)).text();
      for (const line of rawSearchInputs(source))
        violations.push(`${path}:${line}`);
    }
    expect(violations).toEqual([]);
  });
});
