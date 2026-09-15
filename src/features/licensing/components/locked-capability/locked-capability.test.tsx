import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { createPolarPublicLinks, PolarPublicLinkError } from "../../public-links";
import {
  LOCKED_CAPABILITY_EXPLANATION,
  LOCKED_CAPABILITY_SOURCE_INSTRUCTIONS,
  LockedCapability,
  openLockedCapabilityCheckout,
} from "./locked-capability";

const configuredLinks = createPolarPublicLinks(
  {
    desktopCheckout: "https://buy.polar.sh/polar_cl_desktopFixture",
    customerPortal: "https://polar.sh/alfred/portal",
  },
  async () => {},
);

function render(
  links: Parameters<typeof LockedCapability>[0]["links"],
  capabilityName = "Batch runs",
): string {
  return renderToStaticMarkup(
    <LockedCapability capabilityName={capabilityName} links={links} />,
  );
}

describe("locked capability treatment", () => {
  test("states what is locked and that source builds are never locked", () => {
    const markup = render(configuredLinks);
    expect(markup).toContain(">Batch runs</h3>");
    expect(markup).toContain(LOCKED_CAPABILITY_EXPLANATION);
    expect(markup).toContain("building Alfred from source includes every");
    expect(markup).toContain("at no cost");
    expect(markup).toContain("Paid feature");
    expect(markup).toContain("locked-capability-head");
  });

  test("routes to the configured desktop checkout through the allow-list", async () => {
    let opened: string | undefined;
    const links = createPolarPublicLinks(
      {
        desktopCheckout: "https://buy.polar.sh/polar_cl_desktopFixture",
        customerPortal: "https://polar.sh/alfred/portal",
      },
      async (destination) => {
        opened = destination;
      },
    );

    // The component's only side effect goes through the shared seam.
    await openLockedCapabilityCheckout(links);
    expect(opened).toBe("https://buy.polar.sh/polar_cl_desktopFixture");

    const markup = render(links);
    expect(markup).toContain('class="primary"');
    expect(markup).toContain("<button");
    expect(markup).toContain('type="button"');
    expect(markup).toMatch(/aria-label="Buy a license to use Batch runs"/);
  });

  test("without checkout configuration it explains the source-build route", () => {
    const missingLinks = createPolarPublicLinks(
      {},
      async () => {
        throw new PolarPublicLinkError("not_configured");
      },
    );

    const markup = render(missingLinks, "Scheduled triggers");
    expect(markup).not.toContain("<button");
    expect(markup).toContain(LOCKED_CAPABILITY_SOURCE_INSTRUCTIONS);
    expect(markup).toContain("every feature unlocked, permanently");
  });

  test("the unlock control is keyboard reachable with its own name", () => {
    const markup = render(configuredLinks);
    // A native button element: focusable and operable by keyboard without
    // extra wiring, named independently of the visible text.
    expect(markup).toContain("<button");
    expect(markup).toMatch(/aria-label="Buy a license to use Batch runs"/);
  });

  test("carries no dark patterns in any copy it can show", () => {
    for (const markup of [
      render(configuredLinks),
      render(createPolarPublicLinks({}, async () => {})),
    ]) {
      expect(markup.toLowerCase()).not.toContain("hurry");
      expect(markup.toLowerCase()).not.toContain("limited time");
      expect(markup.toLowerCase()).not.toContain("don't lose");
      expect(markup.toLowerCase()).not.toContain("upgrade now");
      expect(markup).not.toContain("!");
    }
  });

  test("keeps one heading per instance so several capabilities coexist", () => {
    const markup =
      render(configuredLinks, "Batch runs") +
      render(configuredLinks, "Scheduled triggers");
    expect(markup).toContain('id="locked-capability-batch-runs"');
    expect(markup).toContain('id="locked-capability-scheduled-triggers"');
  });
});
