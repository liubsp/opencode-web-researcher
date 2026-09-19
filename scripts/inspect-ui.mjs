// Developer diagnostic: inspect controls or open menus in an owned research tab, never send a message.
import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
const binary = resolve("target/debug", process.platform === "win32" ? "web-research.exe" : "web-research");
const { data_directory } = JSON.parse(execFileSync(binary, ["doctor"], {encoding:"utf8"}));
const record = JSON.parse(await readFile(resolve(data_directory,"chrome.json"),"utf8"));
const targets = await (await fetch(`http://127.0.0.1:${record.port}/json/list`)).json();
const target = targets.find(t => process.argv[2] ? t.id === process.argv[2] : t.url.startsWith("https://chatgpt.com/"));
if (!target) throw new Error("No research ChatGPT target found");
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve,reject) => { ws.onopen=resolve; ws.onerror=reject; });
let seq=0;
function command(method,params) {
  return new Promise((resolve,reject) => {
    const id=++seq;
    const timeout=setTimeout(() => reject(new Error("CDP timeout")),10000);
    ws.onmessage=event => {const result=JSON.parse(event.data);if(result.id===id){clearTimeout(timeout);result.error?reject(result.error):resolve(result.result);}};
    ws.send(JSON.stringify({id,method,params}));
  });
}
const actionScript = await readFile(resolve("crates/research-chatgpt/src/scripts/action.js"),"utf8");
const inspectScript = await readFile(resolve("crates/research-chatgpt/src/scripts/inspect.js"),"utf8");
const actions = {"action-tools":{op:"open_tools",argument:null},reasoning:{op:"open_reasoning",argument:null},high:{op:"select",argument:["Extra High","High"]},"mode-search":{op:"select_mode",argument:"Search"}};
if (process.argv[3] === "verify-smoke-deletion") {
  const job = JSON.parse(await readFile(resolve("target/live-smoke.json"),"utf8"));
  if (!job.response?.complete) throw new Error("Smoke response must be archived before verification");
  const {ResearchClient} = await import("../packages/opencode-plugin/dist/client.js");
  const archive = await new ResearchClient(binary).call({op:"archive",project:"web-research-live-smoke",id:job.thread_id});
  if (archive.thread.target !== target.id || archive.thread.state !== "deletion_pending") throw new Error("Not the retired smoke tab");
  await command("Page.navigate",{url:archive.thread.url});
  await command("Runtime.evaluate",{expression:"new Promise(resolve=>setTimeout(resolve,3000))",awaitPromise:true});
}
if (process.argv[3] === "emulate-focus") {
  await command("Emulation.setFocusEmulationEnabled",{enabled:true});
  await command("Runtime.evaluate",{expression:"new Promise(resolve=>setTimeout(resolve,1000))",awaitPromise:true});
}
const expression = process.argv[3] === "response-dom" ? `(() => {const el=document.querySelector('[data-message-author-role="assistant"]');return {html:el?.parentElement.parentElement.parentElement.outerHTML.slice(0,30000),user:document.querySelector('[data-message-author-role="user"]')?.outerHTML,articles:[...document.querySelectorAll('article')].map(a=>({testid:a.dataset.testid,text:a.innerText.slice(0,500)}))};})()`
  : process.argv[3] === "verify-smoke-deletion" ? `({url:location.href,text:document.body.innerText.slice(-2500)})`
  : process.argv[3] === "emulate-focus" ? inspectScript
  : process.argv[3] === "projects" ? `([...document.querySelectorAll('[aria-label="Open project options for Research"]')].map(a=>a.parentElement.outerHTML))`
  : process.argv[3] === "open-research-project" ? `(() => {const options=document.querySelector('[aria-label="Open project options for Research"]');const home=options?.parentElement.querySelector('[aria-label="Open project home"]');home?.click();return !!home})()`
  : process.argv[3] === "turns" ? inspectScript
  : process.argv[3] === "composer"
  ? `document.querySelector('#prompt-textarea')?.outerHTML`
  : actions[process.argv[3]] ? `(${actionScript})(${JSON.stringify(actions[process.argv[3]])})`
  : process.argv[3] === "search-control"
  ? `(() => [...document.querySelectorAll('*')].filter(el=>el.childElementCount===0 && /^(Web search|Deep research)$/.test(el.textContent.trim())).map(el=>el.parentElement.parentElement.parentElement.parentElement.outerHTML.slice(0,8000)))()`
  : process.argv[3] === "menus"
  ? `(() => ({popups:[...document.querySelectorAll('[popover],[role="menu"],[role="dialog"],[data-state="open"]')].map(el=>({html:el.outerHTML.slice(0,12000),text:el.innerText})),tail:document.body.innerText.slice(-4000)}))()`
  : process.argv[3] === "tools"
  ? `(() => { const b=[...document.querySelectorAll('button')].find(b=>/add files and more/i.test(b.getAttribute('aria-label')||'')); b?.click();return !!b; })()`
  : `(() => [...document.querySelectorAll('button,[role="menuitem"],[role="option"],[role="menuitemradio"], [role="menu"]')].filter(el=>el.getClientRects().length && (el.closest('form,[role="menu"],[role="listbox"]') || /composer|model/.test(el.dataset.testid || '') )).map(el=>({tag:el.tagName,role:el.getAttribute('role'),testid:el.dataset.testid,id:el.id,label:el.getAttribute('aria-label'),text:el.innerText,pressed:el.getAttribute('aria-pressed'),state:el.dataset.state,html:el.outerHTML.slice(0,1500)})))()`;
const result = await command("Runtime.evaluate",{expression,returnByValue:true});
console.log(JSON.stringify({target:target.id,result:result.result.value},null,2));
ws.close();
