import { render } from "preact";
import { useState } from "preact/hooks";
import { ProviderConnectionStep } from "./ProviderConnectionStep";
import { nativeConnectionBackend } from "./providerConnection";
import "./styles.css";
const backend = nativeConnectionBackend();
function Preview() {
  const [screen, setScreen] = useState<"intro" | "provider" | "skipped" | "connected">("intro");
  if (screen === "provider") return <ProviderConnectionStep backend={backend} onBack={() => setScreen("intro")} onSkip={() => setScreen("skipped")} onContinue={() => setScreen("connected")} />;
  return <main class="connection-step"><p class="connection-eyebrow">ISOLATED NATIVE PREVIEW</p><h1>{screen === "intro" ? "Provider setup" : screen === "skipped" ? "AI setup skipped." : "Sign-in reviewed."}</h1><p>{screen === "intro" ? "Review Step 3 without changing your desktop setup." : "This preview does not complete native onboarding or enable Research."}</p><button onClick={() => setScreen("provider")}>{screen === "intro" ? "Open provider setup" : "Back to provider setup"}</button></main>;
}
render(<Preview />, document.getElementById("app")!);
