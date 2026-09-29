// SPIKE (#23) STUB. The real lookup is the bunker's, from #24, called by Nitro with a proxy
// signature. Here: BANCWR_SPIKE_MEMBERS="<hex pubkey>:<role>,…".
export function lookupMember(pubkey: string): { role: string } | undefined {
  for (const entry of (process.env.BANCWR_SPIKE_MEMBERS ?? '').split(',')) {
    const [key, role] = entry.split(':')
    if (key === pubkey && role) return { role }
  }
  return undefined
}
