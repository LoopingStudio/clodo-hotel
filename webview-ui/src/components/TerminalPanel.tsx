import { useEffect, useRef, useCallback, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { appBridge } from '../appBridge.js'
import { TranscriptView } from './TranscriptView.js'
import '@xterm/xterm/css/xterm.css'

const DEFAULT_WIDTH = 560
const DEFAULT_HEIGHT = 560
const MIN_WIDTH = 320
const MIN_HEIGHT = 200

interface TerminalPanelProps {
  agentId: number
  agentName: string
  hasPty: boolean
  initialData?: string[]
  onClose: () => void
}

export function TerminalPanel({ agentId, agentName, hasPty, initialData, onClose }: TerminalPanelProps) {
  const termRef = useRef<Terminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const popupRef = useRef<HTMLDivElement>(null)
  const pendingDataRef = useRef<string[]>([])
  const [termReady, setTermReady] = useState(false)

  // Popup position & size
  const [pos, setPos] = useState({ x: -1, y: -1 })
  const [size, setSize] = useState({ w: DEFAULT_WIDTH, h: DEFAULT_HEIGHT })

  // Center on first mount
  useEffect(() => {
    setPos({
      x: Math.max(10, Math.round((window.innerWidth - size.w) / 2)),
      y: Math.max(40, Math.round((window.innerHeight - size.h) / 2)),
    })
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Initialize xterm — delayed to ensure container is in DOM with dimensions
  useEffect(() => {
    if (!hasPty || !containerRef.current || pos.x < 0) return

    // Wait for next frame so the container has been laid out
    const timerId = setTimeout(() => {
      const el = containerRef.current
      if (!el || el.clientWidth === 0 || el.clientHeight === 0) return

      const term = new Terminal({
        cursorBlink: true,
        fontSize: 13,
        fontFamily: "'Menlo', 'Monaco', 'Courier New', monospace",
        theme: {
          background: '#1e1e2e',
          foreground: '#cdd6f4',
          cursor: '#f5e0dc',
          selectionBackground: '#585b70',
          black: '#45475a',
          red: '#f38ba8',
          green: '#a6e3a1',
          yellow: '#f9e2af',
          blue: '#89b4fa',
          magenta: '#f5c2e7',
          cyan: '#94e2d5',
          white: '#bac2de',
          brightBlack: '#585b70',
          brightRed: '#f38ba8',
          brightGreen: '#a6e3a1',
          brightYellow: '#f9e2af',
          brightBlue: '#89b4fa',
          brightMagenta: '#f5c2e7',
          brightCyan: '#94e2d5',
          brightWhite: '#a6adc8',
        },
      })

      const fit = new FitAddon()
      term.loadAddon(fit)
      term.open(el)
      fit.fit()

      // Replay historical data (from parent buffer) + any pending data
      if (initialData) {
        for (const data of initialData) {
          term.write(data)
        }
      }
      for (const data of pendingDataRef.current) {
        term.write(data)
      }
      pendingDataRef.current = []

      appBridge.postMessage({
        type: 'resizePty',
        agentId,
        cols: term.cols,
        rows: term.rows,
      })

      term.onData((data) => {
        appBridge.postMessage({ type: 'writePty', agentId, data })
      })

      const ro = new ResizeObserver(() => {
        requestAnimationFrame(() => {
          fit.fit()
          appBridge.postMessage({
            type: 'resizePty',
            agentId,
            cols: term.cols,
            rows: term.rows,
          })
        })
      })
      ro.observe(el)

      termRef.current = term
      fitRef.current = fit
      setTermReady(true)

      // Store cleanup for this specific instance
      ;(el as unknown as Record<string, unknown>)._cleanup = () => {
        ro.disconnect()
        try { term.dispose() } catch { /* xterm disposal can throw */ }
      }
    }, 100)

    return () => {
      clearTimeout(timerId)
      const el = containerRef.current
      if (el) {
        const cleanup = (el as unknown as Record<string, () => void>)._cleanup
        if (cleanup) cleanup()
      }
      termRef.current = null
      fitRef.current = null
      setTermReady(false)
    }
  }, [agentId, hasPty, pos.x >= 0]) // re-run when positioned

  // Listen for PTY output
  useEffect(() => {
    if (!hasPty) return
    const handler = (e: MessageEvent) => {
      const msg = e.data
      if (!msg || typeof msg !== 'object') return
      if (msg.type === 'ptyOutput' && msg.agentId === agentId) {
        if (termRef.current) {
          termRef.current.write(msg.data)
        } else {
          pendingDataRef.current.push(msg.data)
        }
      }
      if (msg.type === 'ptyExit' && msg.agentId === agentId && termRef.current) {
        // Exit message is handled by parent buffer
      }
    }
    window.addEventListener('message', handler)
    return () => window.removeEventListener('message', handler)
  }, [agentId, hasPty])

  // Focus terminal when ready
  useEffect(() => {
    if (termReady && termRef.current) {
      termRef.current.focus()
    }
  }, [termReady])

  // Drag to move
  const handleDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    const startX = e.clientX
    const startY = e.clientY
    const startPos = { ...pos }

    const onMove = (ev: MouseEvent) => {
      setPos({
        x: Math.max(0, startPos.x + ev.clientX - startX),
        y: Math.max(0, startPos.y + ev.clientY - startY),
      })
    }
    const onUp = () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
    }
    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp)
  }, [pos])

  // Resize from bottom-right corner
  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const startX = e.clientX
    const startY = e.clientY
    const startSize = { ...size }

    const onMove = (ev: MouseEvent) => {
      setSize({
        w: Math.max(MIN_WIDTH, startSize.w + ev.clientX - startX),
        h: Math.max(MIN_HEIGHT, startSize.h + ev.clientY - startY),
      })
    }
    const onUp = () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
    }
    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp)
  }, [size])

  if (pos.x < 0) return null // not positioned yet

  return (
    <div
      ref={popupRef}
      style={{
        position: 'absolute',
        left: pos.x,
        top: pos.y,
        width: size.w,
        height: size.h,
        display: 'flex',
        flexDirection: 'column',
        background: '#1e1e2e',
        border: '2px solid var(--pixel-border)',
        boxShadow: '4px 4px 0px #0a0a14',
        zIndex: 150,
        overflow: 'hidden',
      }}
    >
      {/* Title bar -- draggable */}
      <div
        onMouseDown={handleDragStart}
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '4px 8px',
          borderBottom: '2px solid var(--pixel-border)',
          background: 'var(--pixel-bg)',
          flexShrink: 0,
          cursor: 'move',
          userSelect: 'none',
        }}
      >
        <span style={{ fontSize: '20px', color: 'var(--pixel-text)' }}>
          {hasPty ? 'Terminal' : 'Transcript'} — {agentName}
        </span>
        <button
          onClick={onClose}
          style={{
            background: 'none',
            border: 'none',
            color: 'var(--pixel-text-dim)',
            cursor: 'pointer',
            fontSize: '20px',
            padding: '0 4px',
          }}
        >
          X
        </button>
      </div>

      {/* Content */}
      {hasPty ? (
        <div
          ref={containerRef}
          style={{
            flex: 1,
            minHeight: 0,
            overflow: 'hidden',
          }}
        />
      ) : (
        <TranscriptView agentId={agentId} />
      )}

      {/* Resize handle -- bottom-right corner */}
      <div
        onMouseDown={handleResizeStart}
        style={{
          position: 'absolute',
          right: 0,
          bottom: 0,
          width: 14,
          height: 14,
          cursor: 'nwse-resize',
        }}
      />
    </div>
  )
}
