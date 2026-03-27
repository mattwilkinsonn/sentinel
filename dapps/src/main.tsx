import React from "react";
import ReactDOM from "react-dom/client";
import "./main.css";

import { QueryClient } from "@tanstack/react-query";
import App from "./App.tsx";
import { EveFrontierProvider } from "@evefrontier/dapp-kit";
import { Theme } from "@radix-ui/themes";

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Theme appearance="dark">
      <EveFrontierProvider queryClient={queryClient}>
        <App />
      </EveFrontierProvider>
    </Theme>
  </React.StrictMode>,
);
