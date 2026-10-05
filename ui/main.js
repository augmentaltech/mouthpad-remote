const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

function forwardingState(s) {
  if (!s.connected) return ["idle", "Not connected", "Plug in the MouthPad over USB and connect."];
  if (s.captureError) return ["idle", "Unavailable", "Input capture is not running."];
  if (s.mouthpadOnHost)
    return ["paused", "Paused", "A MouthPad is connected to this computer over Bluetooth, so input stays local."];
  if (s.paused) return ["paused", "Paused", "Input stays on this computer. Press ⌘⇧P to resume."];
  if (!s.focused) return ["idle", "Waiting for focus", "Focus this window to forward input."];
  return ["live", "Forwarding", "Mouse and keyboard go to the MouthPad. Press ⌘⇧P to pause."];
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
$("open-settings").addEventListener("click", () => invoke("open_accessibility_settings"));
$("retry").addEventListener("click", async () => render((current = await invoke("retry_capture"))));

listen("status", (e) => render((current = e.payload)));
refresh();
setInterval(refresh, 500);
