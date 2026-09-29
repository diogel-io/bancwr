// GET /api/version: this frontend's version. Outside /api/bunker/*, so the proxy never sees it;
// the bunker reports its own version on /api/bunker/status. See #35.
// Imported rather than left to Nitro's auto-import, as in bunker/[...].ts.
import { versionOrDefault } from '../utils/version'

export default defineEventHandler(() => ({
  version: versionOrDefault(process.env.BANCWR_VERSION)
}))
