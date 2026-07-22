// Preload: the only bridge between the sandboxed renderer and the main process.
// Exposes a tiny typed surface on `window.komo`; the bearer key stays in main.

import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("komo", {
  connect: () => ipcRenderer.invoke("komo:connect"),
  api: (req: unknown) => ipcRenderer.invoke("komo:api", req),
  chat: (req: unknown) => ipcRenderer.invoke("komo:chat", req),
});
