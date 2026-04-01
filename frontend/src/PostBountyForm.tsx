import {
  type PaginatedObjectsResponse,
  SuiJsonRpcClient,
} from "@mysten/sui/jsonRpc";
import { Transaction } from "@mysten/sui/transactions";
import { Send } from "lucide-solid";
import { createEffect, createSignal, For, onMount, Show } from "solid-js";
import { useWallet } from "./WalletContext";

const CLOCK_OBJECT_ID = "0x6";
const BUILDER_PACKAGE_ID = import.meta.env.VITE_BUILDER_PACKAGE_ID || "";
const BOUNTY_BOARD_ID = import.meta.env.VITE_BOUNTY_BOARD_ID || "";
const EXTENSION_CONFIG_ID = import.meta.env.VITE_EXTENSION_CONFIG_ID || "";
const WORLD_PACKAGE_ID = import.meta.env.VITE_WORLD_PACKAGE_ID || "";
const SUI_RPC_URL =
  import.meta.env.VITE_SUI_RPC_URL || "https://fullnode.testnet.sui.io:443";
const API_BASE = import.meta.env.VITE_API_BASE || "http://localhost:3001";

type CharacterOption = {
  objectId: string;
  inGameId: string;
  name: string;
};

type NameEntry = { id: string; name: string };

type Props = {
  onSuccess: () => void;
};

function friendlyError(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err);
  const lower = msg.toLowerCase();
  if (
    lower.includes("rejected") ||
    lower.includes("denied") ||
    lower.includes("cancelled")
  )
    return "Transaction cancelled.";
  if (
    lower.includes("wrong chain") ||
    lower.includes("wrong network") ||
    lower.includes("chain mismatch")
  )
    return "Wrong network — switch Slush to Testnet and try again.";
  if (
    lower.includes("simulation") ||
    lower.includes("dry run") ||
    lower.includes("move abort")
  )
    return `Transaction simulation failed — make sure you're on Testnet. Details: ${msg}`;
  if (lower.includes("insufficient") && lower.includes("gas"))
    return "Insufficient SUI for gas. Top up your testnet wallet and try again.";
  if (lower.includes("object not found") || lower.includes("not found"))
    return `Object not found — check your IDs are on Testnet. Details: ${msg}`;
  return msg || "Transaction failed.";
}

export function PostBountyForm(props: Props) {
  const { signAndExecuteTransaction, connectedAddress } = useWallet();

  // Poster character (auto-detected from wallet)
  const [characters, setCharacters] = createSignal<CharacterOption[]>([]);
  const [selectedCharacter, setSelectedCharacter] =
    createSignal<CharacterOption | null>(null);
  const [loadingChars, setLoadingChars] = createSignal(false);

  // Target name search
  const [allNames, setAllNames] = createSignal<NameEntry[]>([]);
  const [targetQuery, setTargetQuery] = createSignal("");
  const [targetMatch, setTargetMatch] = createSignal<NameEntry | null>(null);
  const [showSuggestions, setShowSuggestions] = createSignal(false);

  // Reward + duration
  const [rewardSui, setRewardSui] = createSignal("1");
  const [durationHours, setDurationHours] = createSignal("24");

  const [submitting, setSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // Load name map from API
  onMount(async () => {
    try {
      const res = await fetch(`${API_BASE}/api/data`);
      if (!res.ok) return;
      const data = await res.json();
      const names: NameEntry[] = Object.entries(
        (data.names ?? {}) as Record<string, string>,
      ).map(([id, name]) => ({ id, name }));
      setAllNames(names);
    } catch {
      // name lookup unavailable, user can still type an in-game ID manually
    }
  });

  // Load poster's character objects when wallet connects
  createEffect(() => {
    const addr = connectedAddress();
    if (!addr || !WORLD_PACKAGE_ID) return;
    setLoadingChars(true);
    const client = new SuiJsonRpcClient({
      url: SUI_RPC_URL,
      network: "testnet",
    });
    client
      .getOwnedObjects({
        owner: addr,
        filter: {
          StructType: `${WORLD_PACKAGE_ID}::character::Character`,
        },
        options: { showContent: true },
      })
      .then((res: PaginatedObjectsResponse) => {
        const opts: CharacterOption[] = res.data.flatMap((item) => {
          const data = item.data;
          const content = data?.content;
          if (!data?.objectId || !content || content.dataType !== "moveObject")
            return [];
          const fields = content.fields as Record<string, unknown>;
          const inGameId = String(
            (fields.in_game_id as Record<string, unknown>)?.id ??
              fields.in_game_id ??
              "",
          );
          // Look up name from allNames or fall back to ID
          const entry = allNames().find((n) => n.id === inGameId);
          return [
            {
              objectId: data.objectId,
              inGameId,
              name: entry?.name ?? `Character #${inGameId}`,
            },
          ];
        });
        setCharacters(opts);
        if (opts.length === 1) setSelectedCharacter(opts[0]);
      })
      .catch(() => {})
      .finally(() => setLoadingChars(false));
  });

  const filteredNames = () => {
    const q = targetQuery().toLowerCase().trim();
    if (!q || targetMatch()) return [];
    return allNames()
      .filter((n) => n.name.toLowerCase().includes(q))
      .slice(0, 8);
  };

  function selectTarget(entry: NameEntry) {
    setTargetMatch(entry);
    setTargetQuery(entry.name);
    setShowSuggestions(false);
  }

  function clearTarget() {
    setTargetMatch(null);
    setTargetQuery("");
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    const char = selectedCharacter();
    const target = targetMatch();

    if (!char) {
      setError(
        "No character found in your wallet. Make sure you're on Testnet.",
      );
      return;
    }
    if (!target) {
      setError("Select a target pilot.");
      return;
    }

    const rewardFloat = parseFloat(rewardSui());
    if (Number.isNaN(rewardFloat) || rewardFloat <= 0) {
      setError("Enter a valid reward amount.");
      return;
    }
    const rewardMist = BigInt(Math.round(rewardFloat * 1e9));
    const durationMs = BigInt(Number(durationHours()) * 60 * 60 * 1000);

    setSubmitting(true);
    setError(null);

    try {
      const tx = new Transaction();
      const [coin] = tx.splitCoins(tx.gas, [tx.pure.u64(rewardMist)]);
      tx.moveCall({
        target: `${BUILDER_PACKAGE_ID}::bounty_board::post_bounty`,
        arguments: [
          tx.object(BOUNTY_BOARD_ID),
          tx.object(EXTENSION_CONFIG_ID),
          tx.object(char.objectId),
          coin,
          tx.pure.u64(BigInt(target.id)),
          tx.pure.string("stillness"),
          tx.pure.u64(durationMs),
          tx.object(CLOCK_OBJECT_ID),
        ],
      });

      await signAndExecuteTransaction(tx);
      props.onSuccess();
    } catch (err) {
      setError(friendlyError(err));
    } finally {
      setSubmitting(false);
    }
  }

  const canSubmit = () =>
    !!selectedCharacter() && !!targetMatch() && !submitting();

  return (
    <div class="glass-card p-5">
      <h4 class="text-sm tracking-wider" style="margin-bottom:1rem">
        POST NEW BOUNTY
      </h4>

      <form onSubmit={handleSubmit}>
        <div class="flex flex-col gap-3">
          {/* Poster character */}
          <div>
            <p
              class="block text-xs text-text-muted"
              style="margin-bottom:0.25rem"
            >
              Your Character
            </p>
            <Show
              when={!loadingChars()}
              fallback={
                <p class="text-xs text-text-muted">Detecting character…</p>
              }
            >
              <Show
                when={characters().length > 0}
                fallback={
                  <p class="text-xs text-accent-red">
                    No character found in wallet. Make sure you're on Testnet.
                  </p>
                }
              >
                <Show
                  when={characters().length === 1}
                  fallback={
                    <select
                      class="w-full"
                      onChange={(e) => {
                        const c = characters().find(
                          (ch) => ch.objectId === e.currentTarget.value,
                        );
                        setSelectedCharacter(c ?? null);
                      }}
                    >
                      <For each={characters()}>
                        {(c) => <option value={c.objectId}>{c.name}</option>}
                      </For>
                    </select>
                  }
                >
                  <p class="text-sm text-text-primary font-mono">
                    {selectedCharacter()?.name}
                  </p>
                </Show>
              </Show>
            </Show>
          </div>

          {/* Target pilot search */}
          <div style="position:relative">
            <label
              for="target-pilot"
              class="block text-xs text-text-muted"
              style="margin-bottom:0.25rem"
            >
              Target Pilot
            </label>
            <div class="flex gap-2 items-center">
              <input
                id="target-pilot"
                value={targetQuery()}
                onInput={(e) => {
                  setTargetQuery(e.currentTarget.value);
                  setTargetMatch(null);
                  setShowSuggestions(true);
                }}
                onFocus={() => setShowSuggestions(true)}
                onBlur={() => setTimeout(() => setShowSuggestions(false), 150)}
                placeholder="Search pilot name…"
                class="w-full"
                classList={{ "border-accent-cyan": !!targetMatch() }}
              />
              <Show when={targetMatch()}>
                <button
                  type="button"
                  onClick={clearTarget}
                  class="text-xs text-text-muted hover:text-text-primary px-2"
                  style="white-space:nowrap"
                >
                  ✕
                </button>
              </Show>
            </div>
            <Show when={showSuggestions() && filteredNames().length > 0}>
              <div
                class="glass-card"
                style="position:absolute;z-index:50;width:100%;margin-top:2px;padding:0.25rem 0;max-height:200px;overflow-y:auto"
              >
                <For each={filteredNames()}>
                  {(entry) => (
                    <button
                      type="button"
                      class="w-full text-left px-3 py-1.5 text-sm hover:bg-white/5"
                      onMouseDown={() => selectTarget(entry)}
                    >
                      <span class="text-text-primary">{entry.name}</span>
                      <span
                        class="text-text-muted text-xs"
                        style="margin-left:0.5rem"
                      >
                        #{entry.id}
                      </span>
                    </button>
                  )}
                </For>
              </div>
            </Show>
            <Show when={targetMatch()}>
              <p class="text-xs text-text-muted" style="margin-top:0.25rem">
                In-game ID: {targetMatch()?.id}
              </p>
            </Show>
          </div>

          {/* Reward + duration */}
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label
                for="reward-sui"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Reward (SUI)
              </label>
              <input
                id="reward-sui"
                type="number"
                min="0.001"
                step="0.1"
                value={rewardSui()}
                onInput={(e) => setRewardSui(e.currentTarget.value)}
                required
                class="w-full"
              />
            </div>
            <div>
              <label
                for="duration-hours"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Duration (hours)
              </label>
              <input
                id="duration-hours"
                type="number"
                min="1"
                max="168"
                value={durationHours()}
                onInput={(e) => setDurationHours(e.currentTarget.value)}
                required
                class="w-full"
              />
            </div>
          </div>

          <Show when={error()}>
            <p class="text-accent-red text-sm">{error()}</p>
          </Show>

          <button
            type="submit"
            disabled={!canSubmit()}
            class="flex items-center justify-center gap-2 px-4 py-2.5 bg-accent-cyan/10 border border-accent-cyan text-accent-cyan rounded hover:bg-accent-cyan/20 transition-all disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Send size={14} />
            {submitting() ? "POSTING…" : "POST BOUNTY"}
          </button>
        </div>
      </form>
    </div>
  );
}
