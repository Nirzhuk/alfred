import { Icon } from "../../../../components/icon";
import {
  polarPublicLinks,
  type PolarDestination,
  type PolarPublicLinks,
} from "../../public-links";

/**
 * THE one shared treatment for a locked pro capability (Plan 008 Step 3).
 * Presentational only: the caller asks the entitlement resolver whether the
 * capability is available; when it is not, this explains what it is and how
 * to unlock it. It never shames, never counts down, never nags — a quiet
 * statement of fact with a way out.
 */

export type LockedCapabilityProps = {
  /** Human-readable name of the capability being offered. */
  capabilityName: string;
  /** One honest sentence about what the capability does. */
  description?: string;
  /** Injected for tests; defaults to the app-wide allow-listed links. */
  links?: Pick<PolarPublicLinks, "isConfigured" | "open">;
};

const CHECKOUT_DESTINATION: PolarDestination = "desktopCheckout";

/** What the card says about itself, before any route. Payment buys signed
 * builds and convenience; building from source was always free. */
export const LOCKED_CAPABILITY_EXPLANATION =
  "This is part of paid Alfred. A license unlocks it on official builds — and " +
  "building Alfred from source includes every feature at no cost.";

/** Shown when no checkout link is configured in this build. Mirrors the
 * rebuild-from-source wording of `download-latest.ts` so every unlock route
 * in the app says the same thing. */
export const LOCKED_CAPABILITY_SOURCE_INSTRUCTIONS = [
  "No purchase page is configured in this build. To use this capability:",
  "",
  "    git clone https://github.com/Nirzhuk/alfred-workflows",
  "    bun install --frozen-lockfile",
  "    bun run build",
  "",
  "A build you compile yourself has every feature unlocked, permanently.",
  "See docs/building-from-source.md for platform prerequisites.",
].join("\n");

type OpenableLinks = Pick<PolarPublicLinks, "open">;

/** Opens the desktop checkout through the existing allow-listed public-link
 * seam. Test seam: the component's only side effect, kept out of the JSX so
 * colocated tests can observe it without a DOM event simulation. */
export async function openLockedCapabilityCheckout(
  links: OpenableLinks = polarPublicLinks,
): Promise<void> {
  await links.open(CHECKOUT_DESTINATION);
}

export function LockedCapability({
  capabilityName,
  description,
  links = polarPublicLinks,
}: LockedCapabilityProps) {
  const headingId = `locked-capability-${slug(capabilityName)}`;
  const configured = links.isConfigured(CHECKOUT_DESTINATION);

  return (
    <section
      className="settings-card locked-capability"
      aria-labelledby={headingId}
    >
      <div className="locked-capability-head">
        <span className="locked-capability-icon" aria-hidden="true">
          <Icon name="lock" size={18} />
        </span>
        <div className="locked-capability-titles">
          <p className="locked-capability-kicker">Paid feature</p>
          <h3 id={headingId}>{capabilityName}</h3>
        </div>
      </div>
      {description ? <p className="settings-section-copy">{description}</p> : null}
      <p className="locked-capability-explanation">
        {LOCKED_CAPABILITY_EXPLANATION}
      </p>
      {configured ? (
        <div className="locked-capability-action">
          <button
            type="button"
            className="primary"
            aria-label={`Buy a license to use ${capabilityName}`}
            onClick={() => void openLockedCapabilityCheckout(links)}
          >
            Buy Alfred License
          </button>
          <p className="locked-capability-note">
            One-time purchase. Manual runs stay free in every build.
          </p>
        </div>
      ) : (
        <pre className="locked-capability-instructions">
          {LOCKED_CAPABILITY_SOURCE_INSTRUCTIONS}
        </pre>
      )}
    </section>
  );
}

function slug(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}
