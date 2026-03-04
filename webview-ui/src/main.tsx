import { StrictMode, Component } from 'react'
import type { ReactNode, ErrorInfo } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'

class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state: { error: Error | null } = { error: null }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[Clodo Hotel] React error:', error, info.componentStack)
  }

  render() {
    if (this.state.error) {
      const el = document.getElementById('crash-screen')
      if (el) {
        el.style.display = 'block'
        el.innerHTML = `<h1>Clodo Hotel crashed</h1><pre>${this.state.error.stack ?? this.state.error.message}</pre>`
      }
      return null
    }
    return this.props.children
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
)
