// Refuse to start without the settings sign-in needs (#11). See server/utils/auth/config.ts.
import { authConfigProblems } from '../utils/auth/config'

export default defineNitroPlugin(() => {
  const config = useRuntimeConfig()
  const problems = authConfigProblems(config)
  if (problems.length > 0) {
    throw new Error(`Bancwr cannot start:\n- ${problems.join('\n- ')}`)
  }
})
