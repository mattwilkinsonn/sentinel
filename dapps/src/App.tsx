import { Box, Flex, Heading } from "@radix-ui/themes";
import { abbreviateAddress, useConnection } from "@evefrontier/dapp-kit";
import { useCurrentAccount } from "@mysten/dapp-kit-react";
import { BountyBoard } from "./BountyBoard";

function App() {
  const { handleConnect, handleDisconnect } = useConnection();
  const account = useCurrentAccount();

  return (
    <Box style={{ padding: "20px" }}>
      <Flex
        position="sticky"
        px="4"
        py="2"
        direction="row"
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <Heading size="6">Bounty Board</Heading>
        <button
          onClick={() =>
            account?.address ? handleDisconnect() : handleConnect()
          }
        >
          {account ? abbreviateAddress(account?.address) : "Connect Wallet"}
        </button>
      </Flex>

      <div className="divider" />

      {account ? (
        <BountyBoard walletAddress={account.address} />
      ) : (
        <Box style={{ textAlign: "center", padding: "60px 20px" }}>
          <Heading size="4" style={{ marginBottom: "12px" }}>
            EVE Frontier Bounty Board
          </Heading>
          <p style={{ color: "var(--text-secondary)", maxWidth: "500px", margin: "0 auto" }}>
            Post bounties on targets. Hunters claim rewards with killmail proof.
            Connect your wallet to get started.
          </p>
        </Box>
      )}
    </Box>
  );
}

export default App;
