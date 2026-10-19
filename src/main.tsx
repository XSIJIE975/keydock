import { createRoot } from "react-dom/client";

import App from "./App";
import { ToastProvider } from "./components/ui/toast";
import "./styles/globals.css";

const container = document.getElementById("root");
if (!container) throw new Error("root element missing");

createRoot(container).render(
  <ToastProvider>
    <App />
  </ToastProvider>,
);
