import { ref, watch, onMounted } from "vue";
import { getConfig, setConfig } from "../api/tauri";

const THEME_KEY = "rupoo:theme";
export type Theme = "dark" | "light";

const theme = ref<Theme>("dark");

async function loadTheme() {
  try {
    const saved = await getConfig(THEME_KEY);
    if (saved && (saved === "dark" || saved === "light")) {
      theme.value = saved as Theme;
    }
  } catch {
    theme.value = "dark";
  }
}

function applyTheme(t: Theme) {
  document.documentElement.setAttribute("data-theme", t);
  setConfig(THEME_KEY, t).catch(() => {});
}

// Watch for changes
watch(theme, applyTheme);

// Load on mount
onMounted(loadTheme);

export function useTheme() {
  function toggleTheme() {
    theme.value = theme.value === "dark" ? "light" : "dark";
  }

  return {
    theme,
    toggleTheme,
  };
}
