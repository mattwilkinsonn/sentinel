import { Show } from "solid-js";
import { ThreatCard } from "./ThreatCard";
import type { ThreatProfile } from "./types";

type InlineCardProps = {
  profile: ThreatProfile | undefined;
  selectedId: number | null;
  characterId: number;
  onClose: () => void;
};

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
