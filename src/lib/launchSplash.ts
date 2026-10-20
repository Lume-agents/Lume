/** The capsule `app.html` paints before the app loads. The Orb removes it once it is in place. */
const SPLASH_ID = "lume-launch";
const THEME_KEY = "lume:launch-theme";

/** Fades the launch capsule out; true when there was one, so the caller can greet the user once. */
export function dismissLaunchSplash(): boolean {
  const splash = typeof document === "undefined" ? null : document.getElementById(SPLASH_ID);
  if (!splash) return false;
  splash.classList.add("leaving");
  window.setTimeout(() => splash.remove(), 300);
  return true;
}

/** The next launch paints the capsule in the colors the Orb had this time. */
export function rememberLaunchTheme(dark: boolean) {
  try { localStorage.setItem(THEME_KEY, dark ? "dark" : "light"); } catch { /* optional */ }
}
