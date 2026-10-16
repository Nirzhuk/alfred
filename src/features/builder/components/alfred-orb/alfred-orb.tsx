import mustache from "../../../../assets/brand/alfred-mustache.svg?no-inline";

type Props = {
  size?: "xs" | "sm" | "hero";
  /** Spins the ring faster and makes the mustache twirl. */
  busy?: boolean;
};

/** Alfred's presence in the Workflow Builder: the app-icon mustache in a live ring. */
export function AlfredOrb({ size = "sm", busy = false }: Props) {
  return (
    <span
      className={["alfred-orb", `is-${size}`, busy ? "is-busy" : ""].filter(Boolean).join(" ")}
      aria-hidden
    >
      <span className="alfred-orb-face">
        <img className="alfred-orb-mustache" src={mustache} alt="" draggable={false} />
      </span>
    </span>
  );
}
