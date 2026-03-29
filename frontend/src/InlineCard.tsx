import { Show } from "solid-js";
import { ThreatCard } from "./ThreatCard";
import type { ThreatProfile } from "./types";

type InlineCardProps = {
  /** The resolved profile to display, or undefined if it hasn't been found yet. */
  profile: ThreatProfile | undefined;
  /** The currently selected character ID from the parent's state. */
  selectedId: number | null;
  /** The character ID this inline slot belongs to. The card only renders when `selectedId === characterId`. */
  characterId: number;
  onClose: () => void;
};

/**
 * Conditionally renders a `ThreatCard` inline within a list when the row's
 * character matches the global selection. The `Show` guard also checks that
 * the profile has loaded, preventing a flash of empty content.
 */
export function InlineCard(props: InlineCardProps) {
  return (
    <Show when={props.selectedId === props.characterId && props.profile}>
      {(profile) => (
        <div style="margin-top:0.5rem;margin-bottom:0.5rem">
          <ThreatCard profile={profile()} onClose={props.onClose} />
        </div>
      )}
    </Show>
  );
}
