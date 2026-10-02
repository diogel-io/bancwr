// Preparing a profile image for upload (#30). Stills are re-encoded through a canvas, which also
// drops their metadata (camera, location); GIFs are sent as they are, to keep their animation.

export type ImageKind = 'picture' | 'banner'

/** The longest side each kind is scaled down to. */
export const MAX_SIDE: Record<ImageKind, number> = { picture: 512, banner: 1500 }
export const STILL_TYPES = ['image/png', 'image/jpeg', 'image/webp']
export const MAX_STILL_BYTES = 10 * 1024 * 1024
export const MAX_GIF_BYTES = 5 * 1024 * 1024

export class ImageError extends Error {}

/** Why a file cannot be used, or undefined when it can. */
export function checkImage(file: Pick<File, 'type' | 'size'>): string | undefined {
  if (file.type === 'image/gif') {
    return file.size > MAX_GIF_BYTES ? 'GIFs can be up to 5 MB.' : undefined
  }
  if (!STILL_TYPES.includes(file.type)) return 'Choose a PNG, JPEG, WebP or GIF image.'
  return file.size > MAX_STILL_BYTES ? 'Images can be up to 10 MB.' : undefined
}

/** The size to draw at: the longest side at most `max`, never enlarged. */
export function scaledSize(width: number, height: number, max: number): { width: number, height: number } {
  const scale = Math.min(1, max / Math.max(width, height))
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) }
}

export async function prepareImage(file: File, kind: ImageKind): Promise<Blob> {
  const problem = checkImage(file)
  if (problem) throw new ImageError(problem)
  if (file.type === 'image/gif') return file

  let bitmap: ImageBitmap
  try {
    bitmap = await createImageBitmap(file)
  } catch {
    throw new ImageError('This image could not be read.')
  }
  const { width, height } = scaledSize(bitmap.width, bitmap.height, MAX_SIDE[kind])
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  canvas.getContext('2d')?.drawImage(bitmap, 0, 0, width, height)
  bitmap.close()
  const blob = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, 'image/webp', 0.85))
  if (!blob) throw new ImageError('This image could not be converted.')
  return blob
}
