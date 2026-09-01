import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppDialogProvider } from "./components/AppDialog";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppDialogProvider><App /></AppDialogProvider>
  </React.StrictMode>,
);
