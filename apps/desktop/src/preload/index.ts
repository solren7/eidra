// Preload: the only bridge between the sandboxed renderer and the main process.
// Exposes a single method — gateway discovery — on `window.komoBridge`. The
// renderer builds its HttpKomoClient over this resolver; all HTTP goes direct.

import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("komoBridge", {
  gateway: () => ipcRenderer.invoke("komo:gateway"),
});
