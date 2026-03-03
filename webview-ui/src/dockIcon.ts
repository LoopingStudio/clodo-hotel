import { getCharacterSprites } from './office/sprites/spriteData.js'
import { Direction } from './office/types.js'

export type DockIconState = 'idle' | 'active' | 'waiting'

const SIZE = 64
const SCALE = 2

/** Render a 64×64 dock icon based on agent state. Returns raw RGBA as base64. */
export function generateDockIconRgba(state: DockIconState): { data: string; width: number; height: number } | null {
  const sprites = getCharacterSprites(0)
  const sprite = state === 'active'
    ? sprites.typing[Direction.DOWN][0]
    : sprites.walk[Direction.DOWN][1]

  const canvas = document.createElement('canvas')
  canvas.width = SIZE
  canvas.height = SIZE
  const ctx = canvas.getContext('2d')
  if (!ctx) return null

  // Background
  ctx.fillStyle = '#1e1e2e'
  ctx.fillRect(0, 0, SIZE, SIZE)

  // Character sprite centered
  const spriteH = sprite.length
  const spriteW = sprite[0]?.length ?? 0
  const offsetX = Math.floor((SIZE - spriteW * SCALE) / 2)
  const offsetY = Math.floor((SIZE - spriteH * SCALE) / 2)

  for (let row = 0; row < spriteH; row++) {
    for (let col = 0; col < (sprite[row]?.length ?? 0); col++) {
      const color = sprite[row][col]
      if (color) {
        ctx.fillStyle = color
        ctx.fillRect(offsetX + col * SCALE, offsetY + row * SCALE, SCALE, SCALE)
      }
    }
  }

  // State indicator dot — bottom-right, pixel art style
  if (state !== 'idle') {
    const dotColor = state === 'active' ? '#4cff6e' : '#ffd93d'
    const dotBright = state === 'active' ? '#afffbf' : '#fff7b0'
    ctx.fillStyle = dotColor
    ctx.fillRect(SIZE - 14, SIZE - 14, 10, 10)
    ctx.fillStyle = dotBright
    ctx.fillRect(SIZE - 12, SIZE - 12, 4, 4)
  }

  // Raw RGBA → base64 (avoid spread operator for large arrays)
  const imageData = ctx.getImageData(0, 0, SIZE, SIZE)
  const bytes = new Uint8Array(imageData.data.buffer)
  let binary = ''
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i])
  }

  return { data: btoa(binary), width: SIZE, height: SIZE }
}
