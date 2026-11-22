import { afterEach, describe, expect, it } from "vitest";
import { Proposals } from "../src/Proposals";
import { App } from "../src/App";
import type { Backend } from "../src/backend";
import type { ContextPreview, ProposalJob } from "../src/types";
import { check, click, mount, must, q, text, tick, type, waitFor } from "./helpers/dom";
import { StdioBackend } from "./helpers/stdioBackend";
import { makeFixture, type Fixture } from "./helpers/fixtures";
let ui: ReturnType<typeof mount> | null = null;
let fixture: Fixture | null = null;
let real: StdioBackend | null = null;
afterEach(() => { ui?.unmount(); ui=null; real?.close();real=null;fixture?.cleanup();fixture=null; });
const pending = (): ProposalJob => ({ request_id: 7, state: "pending", response: null, error: null });
const success = (): ProposalJob => ({ ...pending(), state: "succeeded", response: { provider:"open_ai",model:"fixture-model",text:"<script>untrusted()</script> Suggest a small refactor.",untrusted:true,usage:{input_tokens:3,output_tokens:7,total_tokens:10} } });
const failure = (code="authentication"): ProposalJob => ({ ...pending(),state:"failed",error:{code,message:"Provider request failed safely.",details:[]} });
async function fill() { await type("proposal-model","fixture-model");await type("proposal-prompt","Explain this design using only this text"); }

describe("TAKO-012 proposal-only workspace", () => {
  it("does no startup I/O and transmits only explicit text and bounded settings", async () => {
    const calls: {cmd:string; args:unknown}[]=[];
    const backend: Backend = {async call(cmd,args) { calls.push({cmd,args});
      if(cmd==="proposal_start")return pending() as never;
      if(cmd==="proposal_poll")return success() as never;
      throw new Error(`Unexpected command ${cmd}`);
    }};
    ui=mount(<Proposals backend={backend} pollMs={10}/>);await tick(30);expect(calls).toEqual([]);
    expect(ui.el.querySelector('input[type="password"]')).toBeNull();
    expect(must<HTMLSelectElement>("proposal-provider").value).toBe("open_ai");
    expect([...must<HTMLSelectElement>("proposal-provider").options].filter(o=>!o.disabled).map(o=>o.value)).toEqual(["open_ai"]);
    await fill();await type("proposal-timeout","12");await type("proposal-tokens","256");await click("proposal-send");
    await waitFor("proposal text",()=>!!q("proposal-result"),3000);
    expect(calls[0]).toEqual({cmd:"proposal_start",args:{provider:"open_ai",model:"fixture-model",prompt:"Explain this design using only this text",timeout_s:12,max_output_tokens:256}});
    expect(calls.every(c=>c.cmd==="proposal_start"||c.cmd==="proposal_poll")).toBe(true);
    expect(text("proposal-text")).toContain("<script>untrusted()</script>");expect(ui.el.querySelector("script")).toBeNull();
    expect(text("proposal-result")).toContain("Untrusted model response");expect(text("proposal-usage")).toContain("10 total");
    expect(q("proposal-cancel")).toBeNull();
  });
  it("cancels only the active request and displays no cancelled response", async () => {
    const calls: {cmd:string;args:unknown}[]=[];let cancelled=false;
    const backend: Backend={async call(cmd,args){calls.push({cmd,args});
      if(cmd==="proposal_start")return pending() as never;
      if(cmd==="proposal_cancel"){cancelled=true;return {...pending(),state:"cancelling"} as never;}
      if(cmd==="proposal_poll")return (cancelled?{...failure("cancelled"),state:"cancelled"}:pending()) as never;
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend} pollMs={10}/>);await fill();await click("proposal-send");
    await waitFor("active request",()=>!!q("proposal-cancel"),3000);expect(must<HTMLButtonElement>("proposal-send").disabled).toBe(true);
    await click("proposal-cancel");await waitFor("cancelled",()=>text("proposal-state")==="cancelled",3000);
    expect(calls.find(c=>c.cmd==="proposal_cancel")).toEqual({cmd:"proposal_cancel",args:{request_id:7}});
    expect(q("proposal-result")).toBeNull();expect(text("proposal-error")).toContain("cancelled");
  });
  it("clears old suggestions on a failed retry and validates request limits", async () => {
    let attempt=0;
    const backend: Backend={async call(cmd){
      if(cmd==="proposal_start"){attempt++;return pending() as never;}
      if(cmd==="proposal_poll")return (attempt===1?success():failure()) as never;
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend} pollMs={10}/>);await fill();
    await type("proposal-timeout","0");expect(must<HTMLButtonElement>("proposal-send").disabled).toBe(true);
    await type("proposal-timeout","30");await click("proposal-send");await waitFor("first result",()=>!!q("proposal-result"),3000);
    await click("proposal-send");await waitFor("failure",()=>!!q("proposal-error"),3000);
    expect(text("proposal-error")).toContain("authentication");expect(q("proposal-result")).toBeNull();
  });
  it("status failures request cancellation and never imply successful completion", async () => {
    let cancelCalled=false;
    const backend: Backend={async call(cmd){
      if(cmd==="proposal_start")return pending() as never;
      if(cmd==="proposal_poll"){if(!cancelCalled)throw new Error("lost IPC");return {...failure("cancelled"),state:"cancelled"} as never;}
      if(cmd==="proposal_cancel"){cancelCalled=true;return {...pending(),state:"cancelling"} as never;}
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend} pollMs={10}/>);await fill();await click("proposal-send");
    await waitFor("cancelled after lost status",()=>text("proposal-state")==="cancelled",3000);
    expect(cancelCalled).toBe(true);expect(q("proposal-result")).toBeNull();
  });
  it("a pending proposal leaves real local planning available with T disabled", async () => {
    fixture=makeFixture();real=new StdioBackend(fixture.data);
    const calls:string[]=[];const be=real;
    const backend: Backend={async call(cmd,args){calls.push(cmd);
      if(cmd==="proposal_start")return pending() as never;
      if(cmd==="proposal_poll"||cmd==="proposal_cancel")return pending() as never;
      return be.call(cmd,args);
    }};
    ui=mount(<App backend={backend}/>);await fill();await click("proposal-send");
    await type("project-path",fixture.project);await click("open-project");
    await waitFor("local project",()=>text("project-name")===fixture!.project,3000);
    await type("goal","Keep local engine independent");await click("create-mission");
    await waitFor("local mission",()=>text("state-chip")==="created",3000);await click("define-plan");
    await waitFor("local plan",()=>!!q("plan-card"),3000);
    expect(must<HTMLInputElement>("provider-radio").disabled).toBe(true);
    expect(must<HTMLInputElement>("perm-fs_write_workspace").checked).toBe(false);
    expect(text("backend-status")).toContain("disabled");expect(calls.some(c=>c.startsWith("backend_")||c==="configure_integration")).toBe(false);
    expect(text("proposal-state")).toBe("pending");
  });
  it("missing credentials fail through real Rust IPC without T or a provider request", async () => {
    fixture=makeFixture();real=new StdioBackend(fixture.data);
    ui=mount(<Proposals backend={real} pollMs={10}/>);await fill();await click("proposal-send");
    await waitFor("native credential refusal",()=>text("proposal-error").includes("missing_credentials"),3000);
    expect(q("proposal-result")).toBeNull();expect(real.count("proposal_poll")).toBe(0);
    const state=await real.call("integration_status",undefined);expect(state.availability).toBe("disabled");
  });
  it("a late cancel acknowledgement cannot replace a newer request", async () => {
    let attempt=0;let cancelled=false;let resolve!: (job:ProposalJob)=>void;
    const backend:Backend={async call(cmd,args){
      if(cmd==="proposal_start"){attempt++;return {...pending(),request_id:attempt} as never;}
      if(cmd==="proposal_cancel"){cancelled=true;return await new Promise<ProposalJob>(r=>{resolve=r;}) as never;}
      if(cmd==="proposal_poll"){
        const id=(args as {request_id:number}).request_id;
        return {...pending(),request_id:id,state:id===1&&cancelled?"cancelled":"pending"} as never;
      }
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend} pollMs={10}/>);await fill();await click("proposal-send");
    await click("proposal-cancel");await waitFor("first cancelled",()=>text("proposal-state")==="cancelled",3000);
    await click("proposal-send");await waitFor("second pending",()=>text("proposal-state")==="pending",3000);
    resolve({...pending(),request_id:1,state:"cancelling"});await tick(30);
    expect(text("proposal-state")).toBe("pending");
  });
  it("a response after unmount is cancelled rather than rendered", async () => {
    let resolve!: (value:ProposalJob)=>void;let cancelId:number|undefined;
    const backend:Backend={async call(cmd,args){
      if(cmd==="proposal_start")return await new Promise<ProposalJob>(r=>{resolve=r;}) as never;
      if(cmd==="proposal_cancel"){cancelId=(args as {request_id:number}).request_id;return pending() as never;}
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend}/>);await fill();await click("proposal-send");ui.unmount();ui=null;
    resolve(pending());await tick(30);expect(cancelId).toBe(7);expect(q("proposal-result")).toBeNull();
  });
});

describe("TAKO-013 explicit project context", () => {
  it("does no context I/O until enabled and sends only a consented preview ID", async () => {
    const calls: {cmd:string;args:unknown}[]=[];
    const backend: Backend={async call(cmd,args){ calls.push({cmd,args});
      if(cmd==="conversation_history") return [] as never;
      if(cmd==="context_preview") return {preview_id:4,project_path:"/project/a",text:"EXACT selected source",bytes:21} as never;
      if(cmd==="workspace_start") return pending() as never;
      if(cmd==="proposal_poll") return pending() as never;
      if(cmd==="proposal_cancel") return pending() as never;
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend} projectPath="/project/a" pollMs={10}/>);
    await tick(30);expect(calls).toEqual([]);
    await check("context-enabled");await fill();await type("context-paths","src/main.rs");
    expect(calls.map(c=>c.cmd)).toEqual(["conversation_history"]);
    expect(must<HTMLButtonElement>("proposal-send").disabled).toBe(true);
    await click("context-preview-button");expect(text("context-preview")).toBe("EXACT selected source");
    expect(calls.find(c=>c.cmd==="context_preview")?.args).toEqual({project_path:"/project/a",paths:["src/main.rs"],prompt:"Explain this design using only this text",include_history:false});
    expect(must<HTMLButtonElement>("proposal-send").disabled).toBe(true);
    await check("context-consent");
    await type("proposal-prompt","Updated question");expect(q("context-preview")).toBeNull();
    expect(must<HTMLButtonElement>("proposal-send").disabled).toBe(true);
    await click("context-preview-button");await check("context-consent");
    await check("context-history");expect(q("context-preview")).toBeNull();
    await click("context-preview-button");await check("context-consent");await click("proposal-send");
    expect(calls.find(c=>c.cmd==="workspace_start")?.args).toEqual({project_path:"/project/a",preview_id:4,consent:true,provider:"open_ai",model:"fixture-model",timeout_s:30,max_output_tokens:1024});
    expect(calls.some(c=>c.cmd==="proposal_start")).toBe(false);
  });
  it("edits revoke consent and a late preview cannot restore it", async () => {
    let resolve!: (p:ContextPreview)=>void;
    const backend:Backend={async call(cmd){
      if(cmd==="conversation_history")return [] as never;
      if(cmd==="context_preview")return await new Promise<ContextPreview>(r=>{resolve=r;}) as never;
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend} projectPath="/project/a"/>);await check("context-enabled");await fill();
    await click("context-preview-button");await type("context-paths","new.rs");
    resolve({preview_id:1,project_path:"/project/a",text:"old snapshot",bytes:12});await tick(30);
    expect(q("context-preview")).toBeNull();expect(must<HTMLButtonElement>("proposal-send").disabled).toBe(true);
  });
  it("switching projects cancels the active request and restores only that project's history", async () => {
    let switchProject!: (path:string)=>void;
    const { useState }=await import("preact/hooks");
    function Host(){const [path,setPath]=useState("/a");switchProject=setPath;return <Proposals backend={backend} projectPath={path} pollMs={10}/>;}
    const cancelled:number[]=[];
    const backend:Backend={async call(cmd,args){
      if(cmd==="conversation_history")return [{prompt:(args as {project_path:string}).project_path,response:"saved answer"}] as never;
      if(cmd==="context_preview")return {preview_id:1,project_path:"/a",text:"source A",bytes:8} as never;
      if(cmd==="workspace_start"||cmd==="proposal_poll")return pending() as never;
      if(cmd==="proposal_cancel"){cancelled.push((args as {request_id:number}).request_id);return pending() as never;}
      throw new Error(cmd);
    }};
    ui=mount(<Host/>);await check("context-enabled");await fill();await click("context-preview-button");await check("context-consent");await click("proposal-send");
    switchProject("/b");await tick(40);
    expect(cancelled).toEqual([7]);expect(text("conversation-history")).toContain("/b");expect(text("conversation-history")).not.toContain("/a");
    expect(q("context-preview")).toBeNull();expect(q("proposal-result")).toBeNull();
    expect(must<HTMLInputElement>("context-history").checked).toBe(false);
    switchProject("/a");await tick(40);expect(text("conversation-history")).toContain("/a");
  });
  it("real IPC previews only a selected file, refuses traversal and requires consent before credential access", async () => {
    fixture=makeFixture();real=new StdioBackend(fixture.data);
    ui=mount(<Proposals backend={real} projectPath={fixture.project}/>);await check("context-enabled");await fill();await type("context-paths","README.md");
    await click("context-preview-button");await waitFor("exact native preview",()=>text("context-preview").includes("base\n"),3000);
    const p=await real.call("context_preview",{project_path:fixture.project,paths:["README.md"],prompt:"Explain",include_history:false});
    await expect(real.call("workspace_start",{project_path:fixture.project,preview_id:p.preview_id,consent:false,provider:"open_ai",model:"fixture",timeout_s:1,max_output_tokens:16})).rejects.toMatchObject({code:"context_consent"});
    await expect(real.call("workspace_start",{project_path:fixture.project,preview_id:p.preview_id,consent:true,provider:"open_ai",model:"fixture",timeout_s:1,max_output_tokens:16})).rejects.toMatchObject({code:"missing_credentials"});
    await type("context-paths","../outside.md");expect(q("context-preview")).toBeNull();await click("context-preview-button");
    await waitFor("path refusal",()=>text("proposal-error").includes("context_path"),3000);
    expect(real.count("workspace_start")).toBe(2);expect(real.count("proposal_start")).toBe(0);
    expect((await real.call("conversation_history",{project_path:fixture.project}))).toEqual([]);
  });
  it("project request errors and cancellation preserve controls and omit failed turns", async () => {
    let cancelled=false;
    const backend:Backend={async call(cmd){
      if(cmd==="conversation_history"||cmd==="conversation_clear")return [] as never;
      if(cmd==="context_preview")return {preview_id:2,project_path:"/a",text:"preview",bytes:7} as never;
      if(cmd==="workspace_start")return pending() as never;
      if(cmd==="proposal_cancel"){cancelled=true;return {...pending(),state:"cancelling"} as never;}
      if(cmd==="proposal_poll")return (cancelled?{...failure("cancelled"),state:"cancelled"}:pending()) as never;
      throw new Error(cmd);
    }};
    ui=mount(<Proposals backend={backend} projectPath="/a" pollMs={10}/>);await check("context-enabled");await fill();await click("context-preview-button");await check("context-consent");await click("proposal-send");await click("proposal-cancel");
    await waitFor("project cancellation",()=>text("proposal-state")==="cancelled",3000);
    expect(text("proposal-error")).toContain("cancelled");expect(q("proposal-result")).toBeNull();expect(text("conversation-history")).toBe("");expect(q("context-preview")).toBeNull();
  });
});
