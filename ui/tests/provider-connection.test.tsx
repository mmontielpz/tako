import { afterEach, describe, expect, it, vi } from "vitest";
import { ProviderConnectionStep } from "../src/ProviderConnectionStep";
import { connectionView, nativeConnectionBackend, type ConnectionBackend, type ConnectionView } from "../src/providerConnection";
import { mount, tick, waitFor } from "./helpers/dom";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
let ui: ReturnType<typeof mount> | null = null;
afterEach(() => { ui?.unmount(); ui = null; vi.clearAllMocks(); });
const id = "a".repeat(48);
const view = (state: ConnectionView["state"] = "AVAILABLE"): ConnectionView => ({ provider:"codex", state, reason:"TEST_FIXTURE_ONLY", attempt_id: state === "AVAILABLE" ? null : id, generation:1,user_code:null,checked_at:state === "CONNECTED" ? 1 : null,execution_authorized:false });
function fixture(initial: ConnectionView = view()) {
 let current = initial;
 const backend: ConnectionBackend = {list:vi.fn(async()=>current),begin:vi.fn(async()=>{current={...view("CONNECTING"),user_code:"ABCD-1234"};return current;}),poll:vi.fn(async()=>current),cancel:vi.fn(async()=>{current=view("AUTH_REQUIRED");return current;}),check:vi.fn(async()=>current)};
 return {backend,set:(v:ConnectionView)=>{current=v;}};
}
function button(label:string){const b=[...document.querySelectorAll("button")].find(b=>b.textContent===label);if(!b)throw Error(label);return b;}
async function press(label:string){button(label).click();await tick();}
async function select(){const r=document.querySelector<HTMLInputElement>("input[type=radio]")!;r.checked=true;r.dispatchEvent(new Event("change",{bubbles:true}));await tick();}
function open(backend:ConnectionBackend,back=vi.fn(),skip=vi.fn(),next=vi.fn()){ui=mount(<ProviderConnectionStep backend={backend} onBack={back} onSkip={skip} onContinue={next}/>);return {back,skip,next};}
describe("native Step 3; backend fixtures are not actual authentication",()=>{
 it("starts without a provider, password field or automatic connection; Skip and Back work",async()=>{
  const f=fixture(),nav=open(f.backend);await tick();expect(document.querySelector<HTMLInputElement>("input")!.checked).toBe(false);expect(button("Connect").disabled).toBe(true);expect(document.querySelector("input[type=password]")).toBeNull();await press("Skip for now");expect(nav.skip).toHaveBeenCalledOnce();await press("Back");expect(nav.back).toHaveBeenCalledOnce();expect(f.backend.begin).not.toHaveBeenCalled();
 });
 it.each(["NOT_INSTALLED","UNSUPPORTED"] as const)("%s is never connectable",async state=>{open(fixture(view(state)).backend);await tick();expect(button("Connect").disabled).toBe(true);expect(document.querySelector<HTMLInputElement>("input")!.disabled).toBe(true);});
 it("Connect is explicit, displays a transient code and offers Cancel",async()=>{
  const f=fixture();open(f.backend);await tick();await select();await press("Connect");expect(f.backend.begin).toHaveBeenCalledOnce();expect(document.body.textContent).toContain("ABCD-1234");expect(document.body.textContent).not.toContain("Connected.");await press("Cancel");expect(f.backend.cancel).toHaveBeenCalledOnce();expect(document.body.textContent).not.toContain("ABCD-1234");
 });
 it("failed native cleanup blocks Skip and redacts raw errors",async()=>{
  const f=fixture();f.backend.cancel=vi.fn(async()=>{throw Error("SENTINEL_NOT_FOR_UI")});const nav=open(f.backend);await tick();await select();await press("Connect");await press("Skip for now");expect(nav.skip).not.toHaveBeenCalled();expect(document.querySelector("[role=alert]")).not.toBeNull();expect(document.body.textContent).not.toContain("SENTINEL");
 });
 it("only fresh native proof enables Continue; the native application retains its session",async()=>{
  const f=fixture(),nav=open(f.backend);await tick();await select();await press("Connect");f.set(view("CONNECTED"));await waitFor("native fixture Connected",()=>document.body.textContent!.includes("Connected. No AI task"));await press("Continue");expect(f.backend.cancel).not.toHaveBeenCalled();expect(nav.next).toHaveBeenCalledOnce();
 });
 it("expired proof requires explicit Check rather than automatic provider requests",async()=>{const f=fixture(view("EXPIRED"));open(f.backend);await tick();expect(document.body.textContent).toContain("needs checking");expect(f.backend.check).not.toHaveBeenCalled();await press("Check connection");expect(f.backend.check).toHaveBeenCalledOnce();});
 it("rejects malformed proof, execution grants and extra credential fields",()=>{
  for(const v of [{...view("CONNECTED"),checked_at:null},{...view("CONNECTED"),execution_authorized:true},{...view(),access_token:"SENTINEL"},{...view("CONNECTING"),user_code:"<script>"}])expect(()=>connectionView(v)).toThrow("could not be verified");
 });
 it("typed bridge exposes no URL, executable, prompt, token or actor parameter",async()=>{
  invoke.mockResolvedValue(view("CONNECTING"));await nativeConnectionBackend().begin(0);expect(invoke).toHaveBeenCalledWith("bridge_call",{cmd:"provider_connection_begin",args:{provider:"codex",intent:"connect_provider",expected_generation:0}});invoke.mockRejectedValue({message:"SENTINEL_SECRET"});await expect(nativeConnectionBackend().list()).rejects.toThrow("Connection service is unavailable");
 });
 it("cancels a late Begin result after unmount",async()=>{
  const f=fixture();let finish!:(v:ConnectionView)=>void;f.backend.begin=vi.fn(()=>new Promise<ConnectionView>(r=>{finish=r;}));open(f.backend);await tick();await select();await press("Connect");ui!.unmount();ui=null;finish(view("CONNECTING"));await tick();expect(f.backend.cancel).toHaveBeenCalledOnce();
 });
 it("keeps Cancel visible when an active attempt loses status connectivity",async()=>{
  const f=fixture();open(f.backend);await tick();await select();await press("Connect");f.backend.poll=vi.fn(async()=>{throw Error("offline")});await waitFor("status failure",()=>!!document.querySelector("[role=alert]"));expect(button("Cancel")).toBeDefined();await press("Cancel");expect(f.backend.cancel).toHaveBeenCalledOnce();
 });

});
