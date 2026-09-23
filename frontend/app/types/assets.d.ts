// Vite resolves an asset import to its URL. Nuxt's generated tsconfig does not pull in
// vite/client, so declare the shapes this app imports.
declare module '*.svg' {
  const src: string
  export default src
}
