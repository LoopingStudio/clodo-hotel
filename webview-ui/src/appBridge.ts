// Tauri bridge: window.__TAURI__ is injected by the Tauri runtime.
// Re-dispatch Tauri events as window 'message' events so
// useAppMessages.ts works unchanged.
const tauriGlobal = (globalThis as Record<string, unknown>)['__TAURI__'] as
  | Record<string, unknown>
  | undefined

const invoke = (tauriGlobal as Record<string, unknown> & { core: { invoke: (cmd: string, args?: unknown) => Promise<unknown> } })?.core.invoke
const listen = (tauriGlobal as Record<string, unknown> & { event: { listen: (event: string, handler: (e: { payload: unknown }) => void) => Promise<() => void> } })?.event.listen

// Buffer postMessage calls until the listener is registered (listen is async)
let listenerReady = false
const pending: unknown[] = []

listen('pa-message', (event: { payload: unknown }) => {
  window.dispatchEvent(new MessageEvent('message', { data: event.payload }))
}).then(() => {
  listenerReady = true
  for (const msg of pending) {
    invoke('handle_message', { message: msg }).catch((err: unknown) => {
      console.error('[Clodo Hotel] Tauri invoke error:', err)
    })
  }
  pending.length = 0
}).catch((err: unknown) => {
  console.error('[Clodo Hotel] Tauri listen error:', err)
})

export const appBridge = {
  postMessage(msg: unknown): void {
    if (!listenerReady) {
      pending.push(msg)
      return
    }
    invoke('handle_message', { message: msg }).catch((err: unknown) => {
      console.error('[Clodo Hotel] Tauri invoke error:', err)
    })
  },
}
