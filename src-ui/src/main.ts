import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./styles/global.css";

// Apply theme immediately before mount to avoid flash
const saved = localStorage.getItem("rupoo:settings");
if (saved) {
  try {
    const settings = JSON.parse(saved);
    if (settings.theme) {
      document.documentElement.setAttribute("data-theme", settings.theme);
    }
  } catch { /* ignore */ }
}

const app = createApp(App);
app.use(createPinia());
app.mount("#app");
