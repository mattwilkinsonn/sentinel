import { abbreviateAddress, useConnection } from "@evefrontier/dapp-kit";
import { useCurrentAccount } from "@mysten/dapp-kit-react";
import { Crosshair } from "lucide-react";
import { BountyBoard } from "./BountyBoard";

function App() {
  const { handleConnect, handleDisconnect } = useConnection();
  const account = useCurrentAccount();

  return (
    <div className="min-h-screen bg-bg-primary">
      {/* Header */}
      <header className="sticky top-0 z-50 glass-card rounded-none border-x-0 border-t-0">
        <div className="max-w-6xl mx-auto px-6 py-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Crosshair className="w-5 h-5 text-accent-cyan" />
            <h1 className="text-lg tracking-wider">BOUNTY BOARD</h1>
          </div>
          <button
            onClick={() => account?.address ? handleDisconnect() : handleConnect()}
            className="px-4 py-2 border border-border-default rounded bg-transparent text-text-primary hover:border-accent-cyan hover:text-accent-cyan transition-all text-sm"
          >
            {account ? abbreviateAddress(account.address) : "CONNECT WALLET"}
          </button>
        </div>
      </header>

      {/* Content */}
      <main className="max-w-6xl mx-auto px-6 py-8">
        {account ? (
          <BountyBoard walletAddress={account.address} />
        ) : (
          <div className="flex flex-col items-center justify-center py-32 text-center">
            <div className="scanline-overlay mb-6">
              <h2 className="text-4xl tracking-wider mb-4">EVE FRONTIER BOUNTY BOARD</h2>
            </div>
            <p className="text-text-secondary max-w-md mb-10 leading-relaxed">
              Post bounties on targets. Hunters claim rewards with killmail proof.
              Connect your wallet to get started.
            </p>
            <button
              onClick={handleConnect}
              className="px-8 py-3 border border-accent-cyan text-accent-cyan rounded hover:bg-accent-cyan/10 transition-all text-sm tracking-wider"
            >
              CONNECT WALLET
            </button>
          </div>
        )}
      </main>
    </div>
  );
}

export default App;
