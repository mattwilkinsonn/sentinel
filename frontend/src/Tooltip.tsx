import { Tooltip as KobalteTooltip } from "@kobalte/core/tooltip";
import type { JSX } from "solid-js";

type TooltipProps = {
  /** Plain-text tooltip content displayed in the floating popover. */
  text: string;
  /**
   * Trigger element. Wrapped in a `<span>` via Kobalte's `as="span"` so the
   * trigger doesn't impose its own block layout on the child.
   */
  children: JSX.Element;
};

/**
 * Thin wrapper around `@kobalte/core` Tooltip with a 300 ms open delay and
 * 6 px gutter. Styling is handled by the global `tooltip-content` and
 * `tooltip-arrow` CSS classes.
 */
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
