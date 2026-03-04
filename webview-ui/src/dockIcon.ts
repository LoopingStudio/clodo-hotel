import { getCharacterSprites } from './office/sprites/spriteData.js'
import { Direction } from './office/types.js'

export type DockIconState = 'idle' | 'active' | 'waiting'

const SIZE = 128
const SCALE = 4

/** Render a dock icon as PNG base64. Transparent background, just the avatar. */
export function generateDockIconPng(state: DockIconState, frameIndex = 0, palette = 0, hueShift = 0): string | null {
  const sprites = getCharacterSprites(palette, hueShift)
  const sprite = state === 'active'
    ? sprites.typing[Direction.DOWN][frameIndex % 2]
    : sprites.walk[Direction.DOWN][1]

  const canvas = document.createElement('canvas')
  canvas.width = SIZE
  canvas.height = SIZE
  const ctx = canvas.getContext('2d')
  if (!ctx) return null

  // Character sprite centered — transparent background
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

  // Export as PNG data URL, strip prefix, return raw base64
  const dataUrl = canvas.toDataURL('image/png')
  return dataUrl.replace('data:image/png;base64,', '')
}
