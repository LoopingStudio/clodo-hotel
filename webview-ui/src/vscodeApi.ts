declare function acquireVsCodeApi(): { postMessage(msg: unknown): void; getState(): unknown; setState(state: unknown): void }

function createAPI() {
  // VS Code webview: acquireVsCodeApi is injected as a global
  const acquireFunc = (globalThis as Record<string, unknown>)['acquireVsCodeApi'] as
    (() => { postMessage(msg: unknown): void }) | undefined

  if (acquireFunc) {
    return acquireFunc()
  }

  // Tauri mode: window.__TAURI__ is injected by the Tauri runtime.
  // Re-dispatch Tauri events as window 'message' events so
  // useExtensionMessages.ts works unchanged.
  const tauriGlobal = (globalThis as Record<string, unknown>)['__TAURI__'] as
    | Record<string, unknown>
    | undefined

  if (tauriGlobal) {
    // Tauri 2: APIs live under window.__TAURI__.core and window.__TAURI__.event
    const invoke = (tauriGlobal as Record<string, unknown> & { core: { invoke: (cmd: string, args?: unknown) => Promise<unknown> } }).core.invoke
    const listen = (tauriGlobal as Record<string, unknown> & { event: { listen: (event: string, handler: (e: { payload: unknown }) => void) => Promise<() => void> } }).event.listen

    // Buffer postMessage calls until the listener is registered (listen is async)
    let listenerReady = false
    const tauriPending: unknown[] = []

    listen('pa-message', (event: { payload: unknown }) => {
      window.dispatchEvent(new MessageEvent('message', { data: event.payload }))
    }).then(() => {
      listenerReady = true
      for (const msg of tauriPending) {
        invoke('handle_message', { message: msg }).catch((err: unknown) => {
          console.error('[Pixel Agents] Tauri invoke error:', err)
        })
      }
      tauriPending.length = 0
    }).catch((err: unknown) => {
      console.error('[Pixel Agents] Tauri listen error:', err)
    })

    return {
      postMessage(msg: unknown): void {
        if (!listenerReady) {
          tauriPending.push(msg)
          return
        }
        invoke('handle_message', { message: msg }).catch((err: unknown) => {
          console.error('[Pixel Agents] Tauri invoke error:', err)
        })
      },
    }
  }

  // Standalone mode: use WebSocket, re-dispatch messages as window events
  // so existing useExtensionMessages.ts code works unchanged.
  const ws = new WebSocket(`ws://${location.host}/ws`)
  ws.onmessage = (e: MessageEvent<string>) => {
    try {
      const data = JSON.parse(e.data) as unknown
      window.dispatchEvent(new MessageEvent('message', { data }))
    } catch { /* ignore */ }
  }
  ws.onerror = () => console.error('[Pixel Agents] WebSocket error')
  ws.onclose = () => console.log('[Pixel Agents] WebSocket closed')

  const pending: string[] = []
  ws.onopen = () => {
    for (const msg of pending) ws.send(msg)
    pending.length = 0
  }

  return {
    postMessage(msg: unknown): void {
      const data = JSON.stringify(msg)
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(data)
      } else {
        pending.push(data)
      }
    },
  }
}

const _g = globalThis as Record<string, unknown>
export const isStandalone = !_g['acquireVsCodeApi'] && !_g['__TAURI__']
export const vscode = createAPI()
