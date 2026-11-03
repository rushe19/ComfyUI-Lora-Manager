import { createApp } from 'vue'
import App from './App.vue'
import { applyTheme, prefs } from './prefs'
import './styles.css'

applyTheme(prefs.theme)
createApp(App).mount('#app')

// Service workers only register on secure origins (https or localhost).
if ('serviceWorker' in navigator && window.isSecureContext) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch((err) => console.warn('SW registration failed', err))
  })
}
