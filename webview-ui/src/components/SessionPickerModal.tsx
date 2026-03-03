import { useState } from 'react'
import type { ProjectSessions, SessionInfo } from '../hooks/useExtensionMessages.js'
import { vscode } from '../vscodeApi.js'

interface SessionPickerModalProps {
  projects: ProjectSessions[]
  onClose: () => void
}

function formatRelativeTime(ms: number): string {
  const diff = Date.now() - ms
  const minutes = Math.floor(diff / 60000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

export function SessionPickerModal({ projects, onClose }: SessionPickerModalProps) {
  const [hoveredSession, setHoveredSession] = useState<string | null>(null)

  const handleAdd = (session: SessionInfo, folderName: string) => {
    if (session.isTracked) return
    vscode.postMessage({
      type: 'addSession',
      sessionId: session.sessionId,
      jsonlFile: session.jsonlFile,
      folderName,
    })
    onClose()
  }

  const untracked = projects.filter(p => p.sessions.some(s => !s.isTracked))

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0,0,0,0.6)',
        zIndex: 1000,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
      onClick={onClose}
    >
      <div
        style={{
          background: 'var(--pixel-bg)',
          border: '2px solid var(--pixel-border)',
          borderRadius: 0,
          boxShadow: '4px 4px 0px #0a0a14',
          minWidth: 340,
          maxWidth: 480,
          maxHeight: '70vh',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div
          style={{
            padding: '10px 14px',
            borderBottom: '2px solid var(--pixel-border)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <span style={{ fontSize: '22px', color: 'var(--pixel-text)', fontWeight: 'bold' }}>
            Add Agent
          </span>
          <button
            onClick={onClose}
            style={{
              background: 'none',
              border: 'none',
              color: 'var(--pixel-text-dim)',
              cursor: 'pointer',
              fontSize: '22px',
              padding: '2px 6px',
            }}
          >
            ✕
          </button>
        </div>

        {/* Content */}
        <div style={{ overflowY: 'auto', padding: '8px 0' }}>
          {untracked.length === 0 ? (
            <div style={{ padding: '20px 16px', color: 'var(--pixel-text-dim)', fontSize: '20px', textAlign: 'center' }}>
              No active Claude sessions found.
              <br />
              <span style={{ fontSize: '18px', opacity: 0.6 }}>
                Start Claude in a terminal to see sessions here.
              </span>
            </div>
          ) : (
            untracked.map(project => (
              <div key={project.projectPath}>
                {/* Project header */}
                <div
                  style={{
                    padding: '6px 14px 4px',
                    fontSize: '18px',
                    color: 'var(--pixel-text-dim)',
                    borderTop: '1px solid var(--pixel-border)',
                    marginTop: 4,
                  }}
                >
                  {project.projectPath.split('/').pop() ?? project.projectPath}
                  <span style={{ opacity: 0.5, marginLeft: 6 }}>
                    {project.projectPath}
                  </span>
                </div>
                {/* Sessions */}
                {project.sessions.filter(s => !s.isTracked).map(session => (
                  <button
                    key={session.sessionId}
                    onClick={() => handleAdd(session, project.dirName)}
                    onMouseEnter={() => setHoveredSession(session.sessionId)}
                    onMouseLeave={() => setHoveredSession(null)}
                    style={{
                      display: 'flex',
                      width: '100%',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      padding: '7px 14px',
                      background: hoveredSession === session.sessionId ? 'var(--pixel-btn-hover-bg)' : 'transparent',
                      border: 'none',
                      borderRadius: 0,
                      cursor: 'pointer',
                      textAlign: 'left',
                    }}
                  >
                    <span style={{ fontSize: '20px', color: 'var(--pixel-text)', fontFamily: 'monospace' }}>
                      {session.sessionId.slice(0, 8)}…
                    </span>
                    <span style={{ fontSize: '18px', color: 'var(--pixel-text-dim)' }}>
                      {formatRelativeTime(session.lastModified)}
                    </span>
                  </button>
                ))}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  )
}
