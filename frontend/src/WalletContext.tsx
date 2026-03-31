import type { Transaction } from "@mysten/sui/transactions";
import {
  isWalletWithRequiredFeatureSet,
  signAndExecuteTransaction as walletSignAndExecute,
} from "@mysten/wallet-standard";
import { getWallets } from "@wallet-standard/core";
import {
  createContext,
  createSignal,
  type JSX,
  onCleanup,
  useContext,
} from "solid-js";

const REQUIRED_FEATURES = [
  "standard:connect",
  "sui:signAndExecuteTransaction",
] as const satisfies `${string}:${string}`[];

// biome-ignore lint/suspicious/noExplicitAny: wallet-standard Wallet type is opaque at runtime
type AnyWallet = any;

export type WalletState = {
  /** All Sui wallets discovered via wallet-standard. */
  wallets: () => AnyWallet[];
  /** Currently connected wallet, or null. */
  connectedWallet: () => AnyWallet | null;
  /** First account address of the connected wallet, or null. */
  connectedAddress: () => string | null;
  /** Connect to a specific wallet. Triggers the wallet's connect popup. */
  connect: (wallet: AnyWallet) => Promise<void>;
  /** Disconnect from the current wallet. */
  disconnect: () => Promise<void>;
  /**
   * Sign and execute a transaction with the connected wallet.
   * Throws if no wallet is connected.
   */
  signAndExecuteTransaction: (tx: Transaction) => Promise<{ digest: string }>;
};

const WalletContext = createContext<WalletState>();

export function WalletProvider(props: { children: JSX.Element }) {
  const registry = getWallets();

  const suiWallets = () =>
    registry
      .get()
      .filter((w) => isWalletWithRequiredFeatureSet(w, [...REQUIRED_FEATURES]));

  const [wallets, setWallets] = createSignal<AnyWallet[]>(suiWallets());
  const [connectedWallet, setConnectedWallet] = createSignal<AnyWallet | null>(
    null,
  );
  const [connectedAddress, setConnectedAddress] = createSignal<string | null>(
    null,
  );

  // Keep wallet list in sync with late-registering extensions.
  const offRegister = registry.on("register", () => setWallets(suiWallets()));
  const offUnregister = registry.on("unregister", () =>
    setWallets(suiWallets()),
  );
  onCleanup(() => {
    offRegister();
    offUnregister();
  });

  async function connect(wallet: AnyWallet) {
    const connectFeature = wallet.features["standard:connect"];
    if (!connectFeature)
      throw new Error(`${wallet.name} does not support standard:connect`);
    const result = await connectFeature.connect();
    const address: string | undefined =
      result?.accounts?.[0]?.address ?? wallet.accounts?.[0]?.address;
    setConnectedWallet(wallet);
    setConnectedAddress(address ?? null);
  }

  async function disconnect() {
    const wallet = connectedWallet();
    if (!wallet) return;
    const disconnectFeature = wallet.features["standard:disconnect"];
    if (disconnectFeature) {
      try {
        await disconnectFeature.disconnect();
      } catch {
        /* ignore */
      }
    }
    setConnectedWallet(null);
    setConnectedAddress(null);
  }

  async function signAndExecuteTransaction(
    tx: Transaction,
  ): Promise<{ digest: string }> {
    const wallet = connectedWallet();
    if (!wallet) throw new Error("No wallet connected");

    const address = connectedAddress();
    if (!address) throw new Error("No account address available");

    const account =
      wallet.accounts.find((a: { address: string }) => a.address === address) ??
      wallet.accounts[0];

    // Transaction implements { toJSON(): Promise<string> } which satisfies
    // SuiSignAndExecuteTransactionInput.transaction.
    const result = await walletSignAndExecute(wallet, {
      transaction: tx,
      account,
      chain: "sui:testnet",
    });

    return { digest: result.digest };
  }

  const state: WalletState = {
    wallets,
    connectedWallet,
    connectedAddress,
    connect,
    disconnect,
    signAndExecuteTransaction,
  };

  return (
    <WalletContext.Provider value={state}>
      {props.children}
    </WalletContext.Provider>
  );
}

export function useWallet(): WalletState {
  const ctx = useContext(WalletContext);
  if (!ctx) throw new Error("useWallet must be used inside WalletProvider");
  return ctx;
}
