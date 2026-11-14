// Bunker responses the proxy streams to the browser as a download, rather than parsing as JSON
// (#96). Only these headers cross from the bunker to the browser; nothing else the bunker sends
// does, and no browser header reaches the bunker (the proxy builds its own).

/** Bunker paths whose successful response is a file. */
export const DOWNLOAD_PATHS: ReadonlySet<string> = new Set(['/api/bunker/logs/export'])

/** The bunker response headers a download keeps. */
export const DOWNLOAD_HEADERS = ['content-type', 'content-disposition'] as const

export function isDownload(pathname: string): boolean {
  return DOWNLOAD_PATHS.has(pathname)
}

/** The headers to send the browser for a download: the allowed ones the bunker set, and no-store. */
export function downloadHeaders(from: Headers): Record<string, string> {
  const headers: Record<string, string> = { 'cache-control': 'no-store' }
  for (const name of DOWNLOAD_HEADERS) {
    const value = from.get(name)
    if (value) headers[name] = value
  }
  return headers
}
