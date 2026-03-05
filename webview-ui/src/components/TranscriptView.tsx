import { useEffect, useState, useRef, useMemo, useCallback } from 'react'
import { marked } from 'marked'
import { appBridge } from '../appBridge.js'

interface TranscriptEntry {
  type: string
  content?: unknown
  message?: { content?: unknown }
}

interface TranscriptViewProps {
  agentId: number
}

marked.setOptions({ breaks: true, gfm: true })

interface ParsedEntry {
  role: 'assistant' | 'user'
  html: string
}

/** Extract displayable text from a JSONL record, returning null if nothing to show */
function parseEntry(entry: TranscriptEntry): ParsedEntry | null {
  const content = entry.message?.content ?? entry.content

  if (entry.type === 'user') {
    // Only show actual user prompts (string content), not tool_result arrays
    if (typeof content === 'string' && content.trim()) {
      return { role: 'user', html: renderMarkdown(content) }
    }
    // Array content = tool_result blocks — skip
    return null
  }

  if (entry.type === 'assistant') {
    if (typeof content === 'string' && content.trim()) {
      return { role: 'assistant', html: renderMarkdown(content) }
    }
    if (Array.isArray(content)) {
      // Extract only text blocks, skip tool_use blocks
      const textParts: string[] = []
      for (const block of content as Array<{ type?: string; text?: string }>) {
        if (block.type === 'text' && block.text?.trim()) {
          textParts.push(block.text)
        }
      }
      if (textParts.length > 0) {
        return { role: 'assistant', html: renderMarkdown(textParts.join('\n\n')) }
      }
    }
    return null
  }

  return null
}

/** Colorize diff-like lines inside <pre><code> blocks */
function colorizeDiffBlocks(html: string): string {
  return html.replace(/<pre><code(?:[^>]*)>([\s\S]*?)<\/code><\/pre>/g, (_match, inner: string) => {
    const lines = inner.split('\n')
    // Heuristic: treat as diff if ≥2 lines start with + or -
    let diffCount = 0
    for (const line of lines) {
      const trimmed = line.replace(/^<span[^>]*>/, '')
      if (/^[+-]/.test(trimmed)) diffCount++
    }
    if (diffCount < 2) return _match

    const colored = lines.map(line => {
      // Check the raw text content (may have html entities)
      const textStart = line.replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>')
      if (/^\+/.test(textStart)) {
        return `<span class="diff-add">${line}</span>`
      }
      if (/^-/.test(textStart)) {
        return `<span class="diff-del">${line}</span>`
      }
      return line
    }).join('\n')
    return `<pre><code>${colored}</code></pre>`
  })
}

function renderMarkdown(text: string): string {
  const html = marked.parse(text, { async: false }) as string
  return colorizeDiffBlocks(html)
}

const baseStyle: React.CSSProperties = {
  padding: '8px 12px',
  margin: '6px 0',
  fontSize: '13px',
  lineHeight: '1.5',
  color: '#cdd6f4',
  fontFamily: "'Menlo', 'Monaco', 'Courier New', monospace",
  overflowWrap: 'break-word',
}

export function TranscriptView({ agentId }: TranscriptViewProps) {
  const [entries, setEntries] = useState<TranscriptEntry[]>([])
  const scrollRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const isAtBottomRef = useRef(true)
  const prevCountRef = useRef(0)

  // Track if user is scrolled to bottom
  const handleScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    isAtBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 30
  }, [])

  // Request transcript on mount / agent change
  useEffect(() => {
    prevCountRef.current = 0
    isAtBottomRef.current = true
    appBridge.postMessage({ type: 'requestTranscript', agentId, limit: 200 })
  }, [agentId])

  // Listen for transcript data
  useEffect(() => {
    const handler = (e: MessageEvent) => {
      const msg = e.data
      if (!msg || typeof msg !== 'object') return
      if (msg.type === 'transcriptData' && msg.agentId === agentId) {
        const lines = msg.lines ?? []
        setEntries(lines)
        // Only auto-scroll if user was already at bottom or new data arrived
        if (isAtBottomRef.current || lines.length !== prevCountRef.current) {
          if (isAtBottomRef.current) {
            requestAnimationFrame(() => bottomRef.current?.scrollIntoView({ behavior: 'instant' }))
          }
        }
        prevCountRef.current = lines.length
      }
    }
    window.addEventListener('message', handler)
    return () => window.removeEventListener('message', handler)
  }, [agentId])

  // Periodic refresh
  useEffect(() => {
    const interval = setInterval(() => {
      appBridge.postMessage({ type: 'requestTranscript', agentId, limit: 200 })
    }, 2000)
    return () => clearInterval(interval)
  }, [agentId])

  const displayEntries = useMemo(() =>
    entries.map(parseEntry).filter(Boolean) as ParsedEntry[],
    [entries],
  )

  return (
    <div ref={scrollRef} onScroll={handleScroll} style={{ flex: 1, overflow: 'auto', padding: '4px 8px' }}>
      <style>{`
        .transcript-md p { margin: 4px 0; }
        .transcript-md h1, .transcript-md h2, .transcript-md h3, .transcript-md h4 {
          margin: 10px 0 4px; color: #89b4fa; font-weight: bold;
        }
        .transcript-md h1 { font-size: 16px; }
        .transcript-md h2 { font-size: 15px; }
        .transcript-md h3 { font-size: 14px; }
        .transcript-md h4 { font-size: 13px; }
        .transcript-md code {
          background: #313244; padding: 2px 5px; border-radius: 3px; font-size: 12px; color: #f5c2e7;
        }
        .transcript-md pre {
          background: #11111b; padding: 10px 12px; margin: 8px 0; overflow-x: auto;
          border: 1px solid #313244; border-radius: 3px; position: relative;
        }
        .transcript-md pre code {
          background: none; padding: 0; color: #cdd6f4; font-size: 12px;
          line-height: 1.6; display: block; white-space: pre;
        }
        .transcript-md ul, .transcript-md ol { margin: 4px 0; padding-left: 20px; }
        .transcript-md li { margin: 2px 0; }
        .transcript-md li > p { margin: 2px 0; }
        .transcript-md strong { color: #f9e2af; }
        .transcript-md em { color: #cba6f7; }
        .transcript-md a { color: #89b4fa; text-decoration: underline; }
        .transcript-md blockquote {
          border-left: 3px solid #585b70; margin: 6px 0; padding: 4px 12px; color: #a6adc8;
          background: rgba(255,255,255,0.02);
        }
        .transcript-md hr { border: none; border-top: 1px solid #313244; margin: 10px 0; }
        .transcript-md table { border-collapse: collapse; margin: 6px 0; width: 100%; }
        .transcript-md th, .transcript-md td {
          border: 1px solid #313244; padding: 4px 8px; font-size: 12px;
        }
        .transcript-md th { background: #1e1e2e; color: #89b4fa; }
        .transcript-md .diff-add { background: rgba(166,227,161,0.12); color: #a6e3a1; display: block; }
        .transcript-md .diff-del { background: rgba(243,139,168,0.12); color: #f38ba8; display: block; }
      `}</style>

      {displayEntries.length === 0 && (
        <div style={{ color: 'var(--pixel-text-dim)', fontSize: '13px', padding: '20px 0', textAlign: 'center', fontFamily: "'Menlo', monospace" }}>
          No transcript data yet...
        </div>
      )}

      {displayEntries.map((entry, i) => (
        <div
          key={i}
          style={{
            ...baseStyle,
            background: entry.role === 'assistant' ? '#2a2a3e' : '#1e3a2e',
            borderLeft: entry.role === 'assistant' ? '3px solid var(--pixel-accent)' : '3px solid #a6e3a1',
          }}
        >
          <div style={{ fontSize: '11px', color: '#7f849c', marginBottom: 4, fontWeight: 'bold' }}>
            {entry.role === 'assistant' ? 'Claude' : 'You'}
          </div>
          <div
            className="transcript-md"
            dangerouslySetInnerHTML={{ __html: entry.html }}
          />
        </div>
      ))}
      <div ref={bottomRef} />
    </div>
  )
}
