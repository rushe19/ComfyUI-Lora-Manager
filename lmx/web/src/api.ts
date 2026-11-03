export interface Health {
  status: string
  name: string
  version: string
}

export async function fetchHealth(base = ''): Promise<Health> {
  const res = await fetch(`${base}/api/health`, { headers: { Accept: 'application/json' } })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  const ct = res.headers.get('content-type') ?? ''
  // An SPA fallback answers unknown paths with HTML; treat that as "server too old", not success.
  if (!ct.includes('application/json')) throw new Error(`unexpected content-type: ${ct}`)
  return (await res.json()) as Health
}
