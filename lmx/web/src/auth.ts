// Optional password: notice "login required" answers from any API call and show the login screen.
import { reactive } from 'vue'

export const auth = reactive({ required: false, enabled: false })

/** Wrap fetch once: a 401 marked `x-lmx-auth: required` means the session is missing or expired. */
export function watchAuth() {
  const orig = window.fetch.bind(window)
  window.fetch = async (...args: Parameters<typeof fetch>) => {
    const res = await orig(...args)
    if (res.status === 401 && res.headers.get('x-lmx-auth') === 'required') auth.required = true
    return res
  }
}

async function post(path: string, body?: unknown, method = 'POST') {
  const res = await fetch(path, {
    method,
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const data = (await res.json().catch(() => ({}))) as { message?: string; enabled?: boolean }
  if (!res.ok) throw new Error(data.message ?? `HTTP ${res.status}`)
  return data
}

export async function refreshAuth() {
  try {
    const s = (await (await fetch('/api/auth/status')).json()) as { enabled: boolean; logged_in: boolean }
    auth.enabled = s.enabled
    auth.required = s.enabled && !s.logged_in
  } catch {
    /* server unreachable: leave as is */
  }
}

export const login = (password: string) => post('/api/auth/login', { password })
export const logout = () => post('/api/auth/logout')
export async function setPassword(current: string | null, next: string) {
  const r = await post('/api/auth/password', { current, new: next }, 'PUT')
  auth.enabled = !!r.enabled
  return r
}
