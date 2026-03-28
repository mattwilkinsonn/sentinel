import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import { QueryClient } from "@tanstack/react-query";
import App from "./App.tsx";
import { EveFrontierProvider } from "@evefrontier/dapp-kit";
import { Toaster } from "sonner";

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <EveFrontierProvider queryClient={queryClient}>
      <App />
      <Toaster
        position="bottom-right"
        theme="dark"
        toastOptions={{
          style: {
            background: 'rgba(11, 11, 11, 0.9)',
            backdropFilter: 'blur(12px)',
            border: '1px solid rgba(250, 250, 229, 0.12)',
            color: '#FAFAE5',
            fontFamily: '"Favorit", monospace',
          },
        }}
      />
    </EveFrontierProvider>
  </React.StrictMode>,
);
