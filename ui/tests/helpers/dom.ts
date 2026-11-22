import { render } from "preact";
import type { VNode } from "preact";

export function mount(node: VNode) {
  const el = document.createElement("div");
  document.body.appendChild(el);
  render(node, el);
  return {
    el,
    unmount() {
      render(null, el);
      el.remove();
    },
  };
}

export const q = (id: string) => document.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
export const text = (id: string) => q(id)?.textContent ?? "";

export function must<T extends HTMLElement = HTMLElement>(id: string): T {
  const e = q(id);
  if (!e) throw new Error(`missing element [data-testid=${id}]`);
  return e as T;
}

export async function type(id: string, value: string) {
  const e = must<HTMLInputElement | HTMLTextAreaElement>(id);
  e.value = value;
  e.dispatchEvent(new Event("input", { bubbles: true }));
  await tick(30); // let Preact re-render
}

export function tick(ms = 20) {
  return new Promise((r) => setTimeout(r, ms));
}

export async function click(id: string) {
  const e = must<HTMLButtonElement>(id);
  if (e.disabled) throw new Error(`button ${id} is disabled`);
  e.click();
  await tick(30);
}

export async function check(id: string, on = true) {
  const e = must<HTMLInputElement>(id);
  e.checked = on;
  e.dispatchEvent(new Event("change", { bubbles: true }));
  await tick(30);
}

export async function waitFor(what: string, f: () => boolean, ms = 30_000) {
  const t = Date.now();
  while (!f()) {
    if (Date.now() - t > ms) throw new Error(`timed out waiting for: ${what}\n--- DOM text ---\n${document.body.textContent?.slice(0, 1500)}`);
    await tick(25);
  }
}
