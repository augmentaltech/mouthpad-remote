const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

const isWindows = navigator.userAgent.includes("Windows");
const SHORTCUT_TEXT = isWindows ? "Ctrl+Shift+P" : "⌘⇧P";
$("shortcut").innerHTML = (isWindows ? ["Ctrl", "Shift", "P"] : ["⌘", "⇧", "P"]).map((k) => `<kbd>${k}</kbd>`).join("");

function forwardingState(s) {
  if (!s.connected) return ["idle", "Not connected", "Plug in the MouthPad over USB and connect."];
  if (s.captureError) return ["idle", "Unavailable", "Input capture is not running."];
  if (s.mouthpadOnHost)
    return ["paused", "Paused", "A MouthPad is connected to this computer over Bluetooth, so input stays local."];
  if (s.paused) return ["paused", "Paused", `Input stays on this computer. Press ${SHORTCUT_TEXT} to resume.`];
  if (!s.focused) return ["idle", "Waiting for focus", "Focus this window to forward input."];
  return ["live", "Forwarding", `Mouse and keyboard go to the MouthPad. Press ${SHORTCUT_TEXT} to pause.`];
}

function render(s) {
  $("device").textContent = s.connected ? s.port : "Not connected";
  const button = $("connect");
  button.textContent = s.connected ? "Disconnect" : "Connect";

  const [cls, label, hint] = forwardingState(s);
  const badge = $("state");
  badge.className = `badge ${cls}`;
  badge.textContent = label;
  $("hint").textContent = hint;
  $("sent").textContent = s.connected
    ? `${s.messagesSent} sent · ${s.acked} acked${s.rejected ? ` · ${s.rejected} rejected` : ""}`
    : "";

  $("ios-guard").checked = s.iosAutocorrectGuard;
  $("capture-error").hidden = !s.captureError;
  $("capture-error-text").textContent = s.captureError || "";
  $("error").textContent = s.lastError || "";
}

let current = null;
async function refresh() {
  current = await invoke("get_status");
  render(current);
}

$("connect").addEventListener("click", async () => {
  try {
    await invoke(current?.connected ? "disconnect" : "connect");
  } catch (e) {
    $("error").textContent = String(e);
  }
  refresh();
});
$("ios-guard").addEventListener("change", (e) => invoke("set_ios_autocorrect_guard", { enabled: e.target.checked }));
$("open-settings").addEventListener("click", () => invoke("open_accessibility_settings"));
$("retry").addEventListener("click", async () => render((current = await invoke("retry_capture"))));

// Keys only reach the page when capture didn't swallow them first (on Windows,
// whenever the page has keyboard focus), so they're forwarded from here too.
function isToggleChord(e) {
  const command = isWindows ? e.ctrlKey && !e.metaKey : e.metaKey && !e.ctrlKey;
  return e.code === "KeyP" && command && e.shiftKey && !e.altKey;
}
for (const type of ["keydown", "keyup"]) {
  window.addEventListener(
    type,
    (e) => {
      const down = type === "keydown";
      if (isToggleChord(e)) {
        e.preventDefault();
        if (down && !e.repeat) invoke("toggle_pause");
        return;
      }
      if (!current?.engaged) return;
      e.preventDefault();
      if (!(down && e.repeat)) invoke("web_key", { code: e.code, down });
    },
    true,
  );
}

listen("status", (e) => render((current = e.payload)));
refresh();
setInterval(refresh, 500);
