import { RefreshCw } from "lucide-solid";
import { Show } from "solid-js";

type LoadingStateProps = {
  /** True while the async data fetch is in flight. */
  loading: boolean;
  /** When true, the loading spinner is suppressed even if `loading` is true (data is already visible). */
  hasData: boolean;
  /** Text shown inside the spinner card while loading; defaults to `"Loading data..."`. */
  loadingText?: string;
  /** Text shown when loading finishes but the dataset is empty; defaults to `"No data yet."`. */
  emptyText?: string;
};

/**
 * Renders a spinner while data is loading (and no data is yet available), or an
 * empty-state message once loading completes with nothing to show.
 * Renders nothing when `hasData` is true, so it can be placed above the real
 * content without conditional wrapping at the call site.
 */
export function LoadingState(props: LoadingStateProps) {
  return (
    <>
      <Show when={props.loading && !props.hasData}>
        <div class="glass-card p-8 text-center">
          <RefreshCw
            size={20}
            class="text-accent-cyan animate-spin mx-auto mb-3"
          />
          <p class="text-text-muted">
            {props.loadingText ?? "Loading data..."}
          </p>
        </div>
      </Show>
      <Show when={!props.loading && !props.hasData}>
        <div class="glass-card p-8 text-center">
          <p class="text-text-secondary">{props.emptyText ?? "No data yet."}</p>
        </div>
      </Show>
    </>
  );
}
