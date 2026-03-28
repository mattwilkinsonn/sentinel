import { Tooltip as KobalteTooltip } from "@kobalte/core/tooltip";
import type { JSX } from "solid-js";

type TooltipProps = {
  text: string;
  children: JSX.Element;
};

export function Tooltip(props: TooltipProps) {
  return (
    <KobalteTooltip gutter={6} openDelay={300}>
      <KobalteTooltip.Trigger as="span">
        {props.children}
      </KobalteTooltip.Trigger>
      <KobalteTooltip.Portal>
        <KobalteTooltip.Content class="tooltip-content">
          <KobalteTooltip.Arrow class="tooltip-arrow" />
          {props.text}
        </KobalteTooltip.Content>
      </KobalteTooltip.Portal>
    </KobalteTooltip>
  );
}
