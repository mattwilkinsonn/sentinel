import { RefreshCw } from "lucide-solid";
import { Show } from "solid-js";

type LoadingStateProps = {
  loading: boolean;
  hasData: boolean;
  loadingText?: string;
  emptyText?: string;
};

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
