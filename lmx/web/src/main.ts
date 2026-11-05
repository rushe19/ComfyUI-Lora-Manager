import { createApp } from 'vue'
import App from './App.vue'
import { refreshAuth, watchAuth } from './auth'
import { atRoot } from './base'
import { applyTheme, prefs } from './prefs'
import './styles.css'

applyTheme(prefs.theme)
watchAuth()
void refreshAuth()
createApp(App).mount('#app')

// Service workers only register on secure origins (https or localhost), and only when lmx is served at the
// site root: below a prefix (inside Forge) the worker's scope would reach into the host app.
if (atRoot && 'serviceWorker' in navigator && window.isSecureContext) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch((err) => console.warn('SW registration failed', err))
  })
  // A new service worker took over an already-open page: reload once so the new build is what runs.
  if (navigator.serviceWorker.controller) {
    let reloaded = false
    navigator.serviceWorker.addEventListener('controllerchange', () => {
      if (reloaded) return
      reloaded = true
      location.reload()
    })
  }
}
