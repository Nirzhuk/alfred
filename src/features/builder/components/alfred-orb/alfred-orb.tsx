import bowTie from "../../../../assets/brand/alfred-bow-tie.svg?no-inline";

type Props = {
  size?: "xs" | "sm" | "hero";
  /** Spins the ring faster and makes the bow tie sway. */
  busy?: boolean;
};

/** Alfred's presence in Speak with Alfred: the app-icon bow tie in a live ring. */
export function AlfredOrb({ size = "sm", busy = false }: Props) {
  return (
    <span
      className={["alfred-orb", `is-${size}`, busy ? "is-busy" : ""].filter(Boolean).join(" ")}
      aria-hidden
    >
      <span className="alfred-orb-face">
        <img className="alfred-orb-bow-tie" src={bowTie} alt="" draggable={false} />
      </span>
    </span>
  );
}
