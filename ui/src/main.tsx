import { render } from "preact";
import { DesktopApp } from "./DesktopApp";
import { tauriBackend } from "./backend";
import "./styles.css";

const root = document.getElementById("app")!;
if ("__TAURI_INTERNALS__" in window) {
  render(<DesktopApp backend={tauriBackend()} />, root);
} else {
  // There is deliberately no mock backend: results must come from the real engine.
  render(
    <main class="workspace" style={{ padding: "2rem" }}>
      <h1>TAKO</h1>
      <p>This interface only works inside the TAKO desktop shell, which provides the engine. No engine is attached to this page.</p>
    </main>,
    root,
  );
}
