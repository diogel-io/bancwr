// An <img> that never sends a Referer (#72): for third-party pictures, so their host isn't told
// which Bancwr shows them.
//
// `referrerpolicy="no-referrer"` alone is not enough on an element whose `src` is set first. The
// browser starts the request as soon as `src` is set, before a later attribute is applied, and
// UAvatar sets `src` before the attributes it passes through: the e2e test saw the Referer sent.
// Here the policy is the first attribute set, so it is in place before `src`.
import { h, type FunctionalComponent } from 'vue'

export const NoReferrerImg: FunctionalComponent = (_props, { attrs }) =>
  h('img', { referrerpolicy: 'no-referrer', ...attrs })
