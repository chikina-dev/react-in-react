import ReactDOM from "react-dom/client";

import { App } from "./App";
import "./style.css";

const container = document.querySelector<HTMLDivElement>("#app");

if (!container) {
  throw new Error("Missing #app root");
}

ReactDOM.createRoot(container).render(<App />);
