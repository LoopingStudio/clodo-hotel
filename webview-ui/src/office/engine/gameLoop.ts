import { MAX_DELTA_TIME_SEC, FRAME_MIN_INTERVAL_MS } from '../../constants.js'

export interface GameLoopCallbacks {
  update: (dt: number) => void
  render: (ctx: CanvasRenderingContext2D) => void
}

export function startGameLoop(
  canvas: HTMLCanvasElement,
  callbacks: GameLoopCallbacks,
): () => void {
  const ctx = canvas.getContext('2d')!
  ctx.imageSmoothingEnabled = false

  let lastTime = 0
  let lastRenderTime = 0
  let rafId = 0
  let stopped = false

  const frame = (time: number) => {
    if (stopped) return
    rafId = requestAnimationFrame(frame)

    // Throttle to target FPS
    if (time - lastRenderTime < FRAME_MIN_INTERVAL_MS) return
    lastRenderTime = time

    const dt = lastTime === 0 ? 0 : Math.min((time - lastTime) / 1000, MAX_DELTA_TIME_SEC)
    lastTime = time

    callbacks.update(dt)

    ctx.imageSmoothingEnabled = false
    callbacks.render(ctx)
  }

  rafId = requestAnimationFrame(frame)

  return () => {
    stopped = true
    cancelAnimationFrame(rafId)
  }
}
